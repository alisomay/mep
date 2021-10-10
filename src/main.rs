mod tui;
use dirs::home_dir;
use std::{
    error::Error,
    fs,
    io::stdin,
    path::{Path, PathBuf},
    sync::mpsc::channel,
    sync::{Arc, Mutex},
    time::Duration,
};
use tui::*;

use koto::{
    runtime::{runtime_error, RuntimeError, Value, ValueList, ValueNumber},
    Koto,
};
use midir::{
    os::unix::{VirtualInput, VirtualOutput},
    MidiInput, MidiOutput,
};

use clap::{App, Arg};

use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};

const SCRIPTS_FOLDER_NAME: &'static str = ".mep";

macro_rules! lock {
    ($i:ident) => {
        $i.lock().unwrap()
    };
}

fn main() -> Result<(), Box<dyn Error>> {
    // Handle unwrap
    let home = home_dir().unwrap();
    let tui = Tui::new();

    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::with_name("port")
                .help("You may give a name to your midi io port")
                .short("p")
                .long("port")
                .value_name("name")
                .takes_value(true),
        )
        .get_matches();

    let mut scripts_folder_path = PathBuf::new();
    scripts_folder_path.push(home.to_owned());
    scripts_folder_path.push(SCRIPTS_FOLDER_NAME);
    let scripts_folder_path_str = &format!("{}", scripts_folder_path.display())[..];

    if !Path::new(scripts_folder_path_str).exists() {
        tui.scripts_folder_not_found()?;
        fs::create_dir(scripts_folder_path_str)?;

        let mut examples_path = PathBuf::new();
        examples_path.push(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("example_scripts");

        copy_directory_contents(
            &format!("{}", examples_path.display()),
            scripts_folder_path_str,
        )?;
    }

    let script_paths = fs::read_dir(scripts_folder_path)?;
    tui.intro()?;

    let shared_available_scripts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let mut available_scripts = lock!(shared_available_scripts);
    
    for (index, path) in script_paths.enumerate() {
        let path_buf = path?.path();
        tui.elements_to_choose(
            &index.to_string(),
            // unwrap is fine here
            &format!("{:?}", path_buf.file_name().unwrap()),
        )?;
        let full_path = format!("{}", path_buf.display());
        available_scripts.push(full_path);
    }

    if available_scripts.len() == 0 {
        tui.empty_scripts_folder()?;
        std::process::exit(0x0);
    }

    let mut choice = String::new();
    let mut chosen_idx: usize;
    let max_idx = available_scripts.len() - 1;
    tui.wait_for_choice()?;
    loop {
        stdin().read_line(&mut choice)?;
        chosen_idx = choice.trim().parse()?;
        // Here check if index is out of bounds.
        if chosen_idx > max_idx {
            tui.ignore_choice(chosen_idx)?;
            continue;
        }
        break;
    }

    let chosen_script =
        fs::read_to_string(available_scripts[chosen_idx].to_owned());

    let shared_koto_runtime = Arc::new(Mutex::new(Koto::default()));
    let mut runtime = lock!(shared_koto_runtime);
    
    runtime.compile(&chosen_script.unwrap())?;
    runtime.set_script_path(Some(PathBuf::from(
        available_scripts[chosen_idx].to_owned(),
    )));

    tui.highlight_and_render(
        &chosen_idx.to_string(),
        available_scripts.to_owned(),
    )?;

    let mep_in = MidiInput::new("mep_input")?;
    let mep_out = MidiOutput::new("mep_output")?;

    let mut input_port_name = String::from("_in");
    let mut output_port_name = String::from("_out");

    let mep_input_port_name = match matches.value_of("port") {
        Some(port_name) => {
            input_port_name.insert_str(0, port_name);
            &input_port_name
        }
        None => "mep_in",
    };

    let mep_output_port_name = match matches.value_of("port") {
        Some(port_name) => {
            output_port_name.insert_str(0, port_name);
            &output_port_name
        }
        None => "mep_out",
    };

    let shared_mep_out_port = Arc::new(Mutex::new(
        mep_out.create_virtual(mep_output_port_name).expect("Couldn't create a virtual output midi port."),
    ));
    
    let shared_koto_runtime_clone = shared_koto_runtime.clone();
    let _mep_in_port = mep_in.create_virtual(
        mep_input_port_name,
        move |_stamp, message, _| {
            
            let _res: Result<(), RuntimeError> = match shared_koto_runtime_clone.try_lock() {
                Ok(mut runtime) => match runtime.prelude().data().get_with_string("midi") {
                    Some(midi_module) => match midi_module {
                        Value::Map(midi_module) => match midi_module.data().get_with_string("listen") {
                            Some(message_listener) => match message_listener {
                                Value::Function(_) => {
                                    // Make a list of koto values from u8 slice.
                                    let message_values = message.iter().map(|byte| Value::Number(ValueNumber::from(byte))).collect::<Vec<Value>>();
                                    // Call "midi.listen" function in script with the midi message.
                                    match runtime.call_function(
                                        message_listener.to_owned(),
                                        &[Value::List(ValueList::from_slice(&message_values))],
                                    ) {
                                        Ok(_) => Ok(()),
                                        Err(e) => {
                                            runtime_error!(format!("{}", e))
                                        }
                                    }
                                }
                                _ => runtime_error!(
                                    "\"midi.listen\" is defined but it is not a function"
                                ),
                            },
                            None => {
                                runtime_error!("Try defining a function as \"midi.listen\"")
                            }
                        },
                        _ => runtime_error!("\"midi\" has been found but it is not a map. Try importing \"midi\" on top of your script like \"import midi\". And do not use the same name further."),
                    },
                    _ => runtime_error!("Try importing \"midi\" on top of your script like \"import midi\""),
                },
                Err(e) => {
                    runtime_error!(format!("{}", e))
                }
            };
        },
        // What is this?
        (),
    ).expect("Couldn't create a virtual input midi port.");


    let mut midi_module = koto_midi::make_module();
    let error_message = "send requires a list of bytes [0 - 255], you may still send malformed messages with this restriction. There will be no problem if you obey the protocol ;)";
    midi_module.add_fn("send", move |vm, args| match vm.get_args(&args) {
        [Value::List(message)] => {
            let msg = message
                .data()
                .iter()
                .map(|value| match value {
                    Value::Number(num) => match num {
                        ValueNumber::I64(byte) if *byte >= 0 && *byte < 256 => Ok(*byte as u8),
                        _ => runtime_error!(error_message),
                    },
                    _ => {
                        runtime_error!(error_message)
                    }
                })
                .collect::<std::result::Result<Vec<u8>, RuntimeError>>();
            let _res: Result<_, RuntimeError> =
                match lock!(shared_mep_out_port).send(&msg.unwrap()[..]) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        runtime_error!(format!("{}", e))
                    }
                };
            Ok(Value::Empty)
        }
        _ => runtime_error!(error_message),
    });

    runtime.prelude().add_map("midi", midi_module);
    runtime.run()?;

    let shared_koto_runtime_clone = shared_koto_runtime.clone();
    let shared_available_scripts_clone = shared_available_scripts.clone();

    // Script watcher
    std::thread::spawn(move || loop {
        let (sender, receiver) = channel();
        let mut watcher = watcher(sender, Duration::from_secs(1)).unwrap();

        let mut watcher_path = home.to_owned();
        watcher_path.push(SCRIPTS_FOLDER_NAME);

        watcher
            .watch(
                format!("{}", watcher_path.display()),
                RecursiveMode::Recursive,
            ).expect("Watching of \"~/.mep\" folder failed.");
        match receiver.recv() {
            Ok(event) => match event {
                DebouncedEvent::Write(path) => match path.extension() {
                        Some(extension) => {
                          if "koto" == &format!("{:?}",extension)[..] {
                            let chosen_script = fs::read_to_string(
                            lock!(shared_available_scripts_clone)[chosen_idx].to_owned(),
                            ).expect("Couldn't read chosen script.");
                            let mut runtime = lock!(shared_koto_runtime_clone);
                            // Handle this unwrap?
                            let chunk = runtime.compile(&chosen_script).unwrap();
                            // Handle this unwrap?
                            runtime.run_chunk(chunk).unwrap();
                          }
                          else {
                              // Error wrong extension
                          }
                        },
                        None => {
                            // Error no extension
                        } 
                    },
                
                _ => {
                    // For debugging.
                    // println!("{:?}", event);
                }
            },
            Err(e) => println!("{:?}", e),
        }
    });

    // We loop here for other choices of scripts.
    loop {
        
        let mut choice = String::new();
        stdin().read_line(&mut choice)?;
        let chosen_idx: usize = choice.trim().parse()?;
        
        // If index is out of bounds.
        if chosen_idx > available_scripts.len() - 1 {
            tui.ignore_choice(chosen_idx)?;
            continue;
        }
        
        let chosen_script =
            fs::read_to_string(available_scripts[chosen_idx].to_owned()).expect("Couldn't read chosen script.");
        // Handle this unwrap?
        let chunk = runtime.compile(&chosen_script).unwrap();
        // Handle this unwrap?
        runtime.run_chunk(chunk).unwrap();
        // Highlight choice
        tui.highlight_and_render(&chosen_idx.to_string(), available_scripts.to_owned())?;
    }
}

// Borrowed from,
// https://stackoverflow.com/questions/26958489/how-to-copy-a-folder-recursively-in-rust
pub fn copy_directory_contents<U: AsRef<Path>, V: AsRef<Path>>(from: U, to: V) -> Result<(), Box<dyn Error>> {
    let mut stack = Vec::new();
    stack.push(PathBuf::from(from.as_ref()));

    let output_root = PathBuf::from(to.as_ref());
    let input_root = PathBuf::from(from.as_ref()).components().count();

    while let Some(working_path) = stack.pop() {
        // println!("process: {:?}", &working_path);

        // Generate a relative path
        let src: PathBuf = working_path.components().skip(input_root).collect();

        // Create a destination if missing
        let dest = if src.components().count() == 0 {
            output_root.clone()
        } else {
            output_root.join(&src)
        };
        if fs::metadata(&dest).is_err() {
            // println!(" mkdir: {:?}", dest);
            fs::create_dir_all(&dest)?;
        }

        for entry in fs::read_dir(working_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                match path.file_name() {
                    Some(filename) => {
                        let dest_path = dest.join(filename);
                        // println!("  copy: {:?} -> {:?}", &path, &dest_path);
                        fs::copy(&path, &dest_path)?;
                    }
                    None => {
                        // println!("failed: {:?}", path);
                    }
                }
            }
        }
    }

    Ok(())
}
