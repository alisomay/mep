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

use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};

const SCRIPTS_FOLDER_NAME: &'static str = ".mep";

fn main() -> Result<(), Box<dyn Error>> {
    let tui = Tui::new();
    let home = home_dir().unwrap();

    let koto = Arc::new(Mutex::new(Koto::default()));

    let mut scripts_folder_path = PathBuf::new();
    scripts_folder_path.push(home.to_owned());
    scripts_folder_path.push(SCRIPTS_FOLDER_NAME);
    let scripts_folder_path_string = &format!("{}", scripts_folder_path.display())[..];

    if !Path::new(scripts_folder_path_string).exists() {
        tui.scripts_folder_not_found()?;
        fs::create_dir(scripts_folder_path_string)?;

        let mut examples_path = PathBuf::new();
        examples_path.push(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("example_scripts");

        copy(
            &format!("{}", examples_path.display()),
            scripts_folder_path_string,
        )?;
    }

    let script_paths = fs::read_dir(scripts_folder_path)?;
    tui.intro()?;

    let available_scripts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    for (index, path) in script_paths.enumerate() {
        let path_buf = path?.path();
        tui.elements_to_choose(
            &index.to_string(),
            &format!("{:?}", path_buf.file_name().unwrap()),
        )?;
        let full_path = format!("{}", path_buf.display());
        available_scripts.lock().unwrap().push(full_path);
    }

    if available_scripts.lock().unwrap().len() == 0 {
        tui.empty_scripts_folder()?;
        std::process::exit(0x0);
    }

    let mut choice = String::new();
    let mut chosen_idx: usize;
    let max_idx = available_scripts.lock().unwrap().len() - 1;
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
        fs::read_to_string(available_scripts.lock().unwrap()[chosen_idx].to_owned());
    koto.lock().unwrap().compile(&chosen_script.unwrap())?;

    let available_scripts_copy = available_scripts.lock().unwrap().to_owned();
    // Highlight choice
    tui.highlight_and_render(&chosen_idx.to_string(), available_scripts_copy)?;

    let chosen_path_buf = Some(PathBuf::from(
        available_scripts.lock().unwrap()[chosen_idx].to_owned(),
    ));
    let runtime = koto.clone();
    koto.lock().unwrap().set_script_path(chosen_path_buf);

    let chosen_script =
        fs::read_to_string(available_scripts.lock().unwrap()[chosen_idx].to_owned())?;
    koto.lock().unwrap().compile(&chosen_script)?;

    let mut midi_module = koto_midi::make_module();

    let mep_in = MidiInput::new("mep_in")?;
    let mep_out = MidiOutput::new("mep_out")?;

    let _mep_in_port = mep_in.create_virtual(
        "mep_in",
        move |_stamp, message, _| {
            let message_value_list = ValueList::default();
            for i in 0..message.len() {
                message_value_list
                    .data_mut()
                    .push(Value::Number(ValueNumber::from(message[i])));
            }

            let _res: Result<(), RuntimeError> = match runtime.try_lock() {
                Ok(mut g) => match g.prelude().data().get_with_string("midi") {
                    Some(midi_map) => match midi_map {
                        Value::Map(map) => match map.data().get_with_string("listen") {
                            Some(listener) => match listener {
                                Value::Function(_) => {
                                    match g.call_function(
                                        listener.clone(),
                                        &[Value::List(message_value_list)],
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
                        // Handle these
                        _ => unreachable!(),
                    },
                    // Handle these
                    _ => unreachable!(),
                },
                Err(e) => {
                    runtime_error!(format!("{}", e))
                }
            };
        },
        (),
    )?;

    let mep_out_port = Arc::new(Mutex::new(mep_out.create_virtual("mep_out").unwrap()));
    midi_module.add_fn("send", move |vm, args| match vm.get_args(&args) {
        [Value::List(message)] => {
            let msg = message
                .data()
                .iter()
                .map(|v| match v {
                    Value::Number(num) => match num {
                        // Truncate.
                        ValueNumber::I64(midi_byte) if *midi_byte >= 0 => Ok(*midi_byte as u8),
                        _ => runtime_error!("send requires a list of positive integers"),
                    },
                    _ => {
                        runtime_error!("send requires a list of positive integers")
                    }
                })
                .collect::<std::result::Result<Vec<u8>, RuntimeError>>();
            let _res: Result<(), RuntimeError> =
                match mep_out_port.lock().unwrap().send(&msg.unwrap()[..]) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        runtime_error!(format!("{}", e))
                    }
                };
            Ok(Value::Empty)
        }
        _ => runtime_error!("send requires a list of positive integers"),
    });

    koto.lock().unwrap().prelude().add_map("midi", midi_module);
    koto.lock().unwrap().run()?;

    // Watcher
    let runtime = koto.clone();
    let available_scripts_clone = available_scripts.clone();
    std::thread::spawn(move || loop {
        let (tx, rx) = channel();
        let mut watcher = watcher(tx, Duration::from_secs(1)).unwrap();

        let mut watcher_path = home.to_owned();
        watcher_path.push(SCRIPTS_FOLDER_NAME);

        watcher
            .watch(
                format!("{}", watcher_path.display()),
                RecursiveMode::Recursive,
            )
            .unwrap();
        match rx.recv() {
            Ok(event) => match event {
                DebouncedEvent::Write(path) if path.extension().unwrap() == "koto" => {
                    let chosen_script = fs::read_to_string(
                        available_scripts_clone.lock().unwrap()[chosen_idx].to_owned(),
                    );
                    let chunk = runtime.lock().unwrap().compile(&chosen_script.unwrap());
                    runtime.lock().unwrap().run_chunk(chunk.unwrap()).unwrap();
                }
                _ => {
                    // println!("{:?}", event);
                }
            },
            Err(e) => println!("watch error: {:?}", e),
        }
    });

    // We can then always loop here for other choices of processors.

    loop {
        let mut choice = String::new();
        stdin().read_line(&mut choice)?;
        let chosen_idx: usize = choice.trim().parse()?;
        // Here check if index is out of bounds.
        if chosen_idx > available_scripts.lock().unwrap().len() - 1 {
            tui.ignore_choice(chosen_idx)?;
            continue;
        }
        let chosen_script =
            fs::read_to_string(available_scripts.lock().unwrap()[chosen_idx].to_owned());
        let chunk = koto.lock().unwrap().compile(&chosen_script.unwrap());
        koto.lock().unwrap().run_chunk(chunk.unwrap()).unwrap();

        let available_scripts = available_scripts.lock().unwrap().to_owned();
        // Highlight choice
        tui.highlight_and_render(&chosen_idx.to_string(), available_scripts)?;
    }

    // unreachable
}

// https://stackoverflow.com/questions/26958489/how-to-copy-a-folder-recursively-in-rust
pub fn copy<U: AsRef<Path>, V: AsRef<Path>>(from: U, to: V) -> Result<(), Box<dyn Error>> {
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
