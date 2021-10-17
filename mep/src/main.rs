mod tui;
use dirs::home_dir;
use std::{
    error::Error,
    fs,
    io::stdin,
    path::{Path, PathBuf},
    sync::mpsc::channel,
    sync::{
        mpsc::{Receiver, TryRecvError},
        Arc, Mutex,
    },
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

const SCRIPTS_FOLDER_NAME: &str = ".mep";

macro_rules! lock {
    ($i:ident) => {
        $i.lock().unwrap()
    };
}

fn get_scripts_folder_path(home: &str) -> PathBuf {
    let mut scripts_folder_path = PathBuf::new();
    scripts_folder_path.push(&home);
    scripts_folder_path.push(SCRIPTS_FOLDER_NAME);
    scripts_folder_path
}

enum MainToTuiMessage {
    Intro,
    ListScripts(String, String),
    WaitForChoice,
    IgnoreChoice,
    HighlightAndRender(String, Vec<String>),
    ErrorInScript(String, String),
    Clear,
}
enum WatcherToMainMessage {
    Change(PathBuf),
}

fn main() -> Result<(), Box<dyn Error>> {
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
        .arg(
            Arg::with_name("home")
                .help("If \"mep\" couldn't determine your home directory, to help it please run it with \"--home <absolute-path-to-your-home-directory>\"")
                .long("home")
                .value_name("home")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("clean")
                .help("Remove \"~/.mep\" directory.")
                .long("clean")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("reset")
                .help("Remove \"~/.mep\" folder's contents and populate with example scripts.")
                .long("reset")
                .takes_value(false),
        )
        .get_matches();

    let tui = Arc::new(Mutex::new(Tui::new()));
    let (to_tui, from_main) = channel::<MainToTuiMessage>();
    let tui_clone = tui.clone();

    // Thread for tui
    std::thread::spawn(move || loop {
        use MainToTuiMessage::*;
        // std::thread::sleep(std::time::Duration::from_millis(10));
        match from_main.recv() {
            Ok(message) => match message {
                Intro => {
                    lock!(tui_clone).intro().unwrap();
                }
                ListScripts(index, file_name) => {
                    lock!(tui_clone)
                        .elements_to_choose(&index, &file_name)
                        .unwrap();
                }
                WaitForChoice => {
                    lock!(tui_clone).wait_for_choice().unwrap();
                }
                IgnoreChoice => {
                    lock!(tui_clone).ignore_choice().unwrap();
                }
                Clear => {
                    lock!(tui_clone).clear().unwrap();
                }
                HighlightAndRender(chosen_index, available_scripts) => {
                    lock!(tui_clone)
                        .highlight_and_render(&chosen_index, &available_scripts)
                        .unwrap();
                }
                ErrorInScript(info, err) => {
                    lock!(tui_clone).show_error(&info).unwrap();
                    eprintln!("{}", err);
                }
            },
            Err(_e) => {
                // dbg!(e);
            }
        }
    });

    let home = match home_dir() {
        Some(dir) => dir,
        None => match matches.value_of("home") {
            Some(path) => PathBuf::from(path),
            None => {
                lock!(tui).no_home()?;
                std::process::exit(0x1);
            }
        },
    };

    let scripts_folder_path = get_scripts_folder_path(home.to_str().unwrap());
    let scripts_folder_path_str = scripts_folder_path.to_str().unwrap();

    if matches.is_present("clean") {
        fs::remove_dir_all(scripts_folder_path_str)?;
        lock!(tui).removed_scripts_folder()?;
        std::process::exit(0x0);
    }

    if matches.is_present("reset") {
        fs::remove_dir_all(scripts_folder_path_str)?;
        lock!(tui).reset_scripts_folder()?;

        let mut examples_path = PathBuf::new();
        examples_path.push(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(examples_path.to_str().unwrap(), scripts_folder_path_str)?;
    }

    if !Path::new(scripts_folder_path_str).exists() {
        lock!(tui).scripts_folder_not_found()?;

        let mut examples_path = PathBuf::new();
        examples_path.push(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(
            &format!("{}", examples_path.display()),
            scripts_folder_path_str,
        )?;
    }

    let script_paths = fs::read_dir(&scripts_folder_path)?;
    let shared_available_scripts: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(vec![]));
    let mut available_scripts = lock!(shared_available_scripts);

    to_tui.send(MainToTuiMessage::Clear)?;
    to_tui.send(MainToTuiMessage::Intro)?;

    for (index, path) in script_paths.enumerate() {
        let path_buf = path?.path();
        to_tui.send(MainToTuiMessage::ListScripts(
            index.to_string(),
            format!("{:?}", path_buf.file_name().unwrap()),
        ))?;
        let full_path = format!("{}", path_buf.display());
        available_scripts.push(full_path);
    }

    if available_scripts.len() == 0 {
        lock!(tui).empty_scripts_folder()?;
        std::process::exit(0x0);
    }

    // Thread for the script watcher
    let (to_main, from_watcher) = channel::<WatcherToMainMessage>();
    std::thread::spawn(move || loop {
        let (sender, receiver) = channel();
        let mut watcher = watcher(sender, Duration::from_secs(1)).unwrap();

        let mut watcher_path = home.clone();
        watcher_path.push(SCRIPTS_FOLDER_NAME);

        watcher
            .watch(
                format!("{}", watcher_path.display()),
                RecursiveMode::Recursive,
            )
            .expect("Watch of \"~/.mep\" folder failed.");
        match receiver.recv() {
            Ok(event) => match event {
                DebouncedEvent::Write(path) | DebouncedEvent::NoticeWrite(path) => {
                    match path.extension() {
                        Some(extension) => match extension.to_str() {
                            Some(extension) if "koto" == extension => {
                                to_main.send(WatcherToMainMessage::Change(path)).unwrap();
                            }
                            Some(_) => {
                                dbg!("Wrong ext?");
                            }
                            None => {
                                dbg!("Problem converting osstr to str");
                                // Error wrong extension
                            }
                        },
                        None => {
                            dbg!("No ext?");
                            // Error no extension
                        }
                    }
                }
                _ => {
                    // For debugging.
                    println!("{:?}", event);
                }
            },
            Err(e) => println!("{:?}", e),
        }
    });

    let mut choice = String::new();
    let mut chosen_idx: usize;
    let max_idx = available_scripts.len() - 1;
    to_tui.send(MainToTuiMessage::WaitForChoice)?;

    loop {
        stdin().read_line(&mut choice)?;
        chosen_idx = match choice.trim().parse() {
            Ok(idx) => idx,
            Err(_) => {
                choice.clear();
                to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                continue;
            }
        };
        // Here check if index is out of bounds.
        if chosen_idx > max_idx {
            choice.clear();
            to_tui.send(MainToTuiMessage::IgnoreChoice)?;
            continue;
        }

        break;
    }

    let chosen_script =
        fs::read_to_string(&available_scripts[chosen_idx]).expect("Couldn't read chosen script.");
    let chosen_script_path = available_scripts[chosen_idx].clone();

    let shared_koto_runtime = Arc::new(Mutex::new(Koto::default()));
    let shared_koto_runtime_clone = shared_koto_runtime.clone();
    let mut runtime = lock!(shared_koto_runtime_clone);
    runtime.set_script_path(Some(PathBuf::from(&available_scripts[chosen_idx])));

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
        mep_out
            .create_virtual(mep_output_port_name)
            .expect("Couldn't create a virtual output midi port."),
    ));

    let mut midi_module = koto_midi::make_module();
    let send_error_message = "send requires a list of bytes [0 - 255], you may still send malformed messages with this restriction. There will be no problem if you obey the protocol ;)";
    midi_module.add_fn("send", move |vm, args| match vm.get_args(args) {
        [Value::List(message)] => {
            let msg = message
                .data()
                .iter()
                .map(|value| match value {
                    Value::Number(num) => match num {
                        ValueNumber::I64(byte) if *byte >= 0 && *byte < 256 => Ok(*byte as u8),
                        _ => runtime_error!(send_error_message),
                    },
                    _ => {
                        runtime_error!(send_error_message)
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
        _ => runtime_error!(send_error_message),
    });

    let mut prelude = runtime.prelude();
    prelude.add_map("midi", midi_module);
    prelude.add_value("random", koto_random::make_module());

    try_compile(
        &to_tui,
        &from_watcher,
        chosen_script,
        chosen_script_path,
        &mut runtime,
    )?;

    to_tui.send(MainToTuiMessage::HighlightAndRender(
        chosen_idx.to_string(),
        (*available_scripts).clone(),
    ))?;

    mep_in.create_virtual(
            mep_input_port_name,
            move |_stamp, message, _| {
                let _res: Result<(), RuntimeError> = match shared_koto_runtime.try_lock() {
                    Ok(mut runtime) => match runtime.prelude().data().get_with_string("midi") {
                        Some(midi_module) => match midi_module {
                            Value::Map(midi_module) => match midi_module.data().get_with_string("listen") {
                                Some(message_listener) => match message_listener {
                                    Value::Function(_) => {
                                        // Make a list of koto values from u8 slice.
                                        let message_values = message.iter().map(|byte| Value::Number(ValueNumber::from(byte))).collect::<Vec<Value>>();
                                        // Call "midi.listen" function in script with the midi message.
                                        match runtime.call_function(
                                            message_listener.clone(),
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

    runtime.run()?;
    fn spawn_stdin_channel() -> Receiver<String> {
        let (stdin_to_main, from_stdin) = channel::<String>();
        std::thread::spawn(move || loop {
            let mut choice = String::new();
            stdin().read_line(&mut choice).unwrap();
            stdin_to_main.send(choice).unwrap();
        });
        from_stdin
    }

    let stdin_channel = spawn_stdin_channel();

    // Enter main loop.
    loop {
        match stdin_channel.try_recv() {
            Ok(mut choice) => {
                chosen_idx = match choice.clone().trim().parse() {
                    Ok(idx) => idx,
                    Err(_) => {
                        choice.clear();
                        to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                        continue;
                    }
                };

                // Here check if index is out of bounds.
                if chosen_idx > max_idx {
                    choice.clear();
                    to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                    continue;
                }

                let chosen_script_path = available_scripts[chosen_idx].clone();
                let chosen_script =
                    fs::read_to_string(&chosen_script_path).expect("Couldn't read chosen script.");

                try_compile(
                    &to_tui,
                    &from_watcher,
                    chosen_script,
                    chosen_script_path.clone(),
                    &mut runtime,
                )?;

                // Highlight choice
                to_tui.send(MainToTuiMessage::HighlightAndRender(
                    chosen_idx.to_string(),
                    (*available_scripts).clone(),
                ))?;
            }
            Err(TryRecvError::Empty) => match from_watcher.try_recv() {
                Ok(message) => {
                    #[allow(irrefutable_let_patterns)]
                    if let WatcherToMainMessage::Change(path) = message {
                        loop {
                            let chosen_script_path = path.to_str().unwrap().to_string();
                            let chosen_script = fs::read_to_string(&chosen_script_path)
                                .expect("Couldn't read chosen script.");
                            match try_compile(
                                &to_tui,
                                &from_watcher,
                                chosen_script,
                                chosen_script_path,
                                &mut runtime,
                            ) {
                                Ok(_) => {
                                    // Script fixed.
                                    // Highlight choice
                                    to_tui.send(MainToTuiMessage::HighlightAndRender(
                                        chosen_idx.to_string(),
                                        (*available_scripts).clone(),
                                    ))?;
                                    break;
                                }
                                Err(_) => {
                                    // Didn't work out try one more time.
                                    continue;
                                }
                            }
                        }
                    }
                }
                Err(_e) => {
                    continue;
                }
            },
            Err(TryRecvError::Disconnected) => panic!("Channel disconnected"),
        }
    }
}

fn try_compile(
    to_tui: &std::sync::mpsc::Sender<MainToTuiMessage>,
    from_watcher: &Receiver<WatcherToMainMessage>,
    chosen_script: String,
    chosen_script_path: String,
    runtime: &mut Koto,
) -> Result<(), Box<dyn Error>> {
    match runtime.compile(&chosen_script) {
        Ok(chunk) => match runtime.run_chunk(chunk) {
            Ok(_) => Ok(()),
            Err(e) => {
                to_tui.send(MainToTuiMessage::Clear)?;
                to_tui.send(MainToTuiMessage::ErrorInScript(
                    chosen_script_path,
                    format!("{:?}", e),
                ))?;
                loop {
                    match from_watcher.recv() {
                        Ok(message) => {
                            #[allow(irrefutable_let_patterns)]
                            if let WatcherToMainMessage::Change(path) = message {
                                let chosen_script_path = path.to_str().unwrap().to_string();
                                let chosen_script = fs::read_to_string(&chosen_script_path)
                                    .expect("Couldn't read chosen script.");
                                match try_compile(
                                    to_tui,
                                    from_watcher,
                                    chosen_script,
                                    chosen_script_path,
                                    runtime,
                                ) {
                                    Ok(_) => {
                                        // Script fixed.
                                        return Ok(());
                                    }
                                    Err(_) => {
                                        // Didn't work out try one more time.
                                        continue;
                                    }
                                }
                            }
                        }
                        Err(_e) => {}
                    }
                }
            }
        },
        Err(e) => {
            // Compile time error.
            to_tui.send(MainToTuiMessage::Clear)?;
            to_tui.send(MainToTuiMessage::ErrorInScript(
                chosen_script_path,
                format!("{:?}", e),
            ))?;
            loop {
                match from_watcher.recv() {
                    Ok(message) => {
                        #[allow(irrefutable_let_patterns)]
                        if let WatcherToMainMessage::Change(path) = message {
                            let chosen_script_path = path.to_str().unwrap().to_string();
                            let chosen_script = fs::read_to_string(&chosen_script_path)
                                .expect("Couldn't read chosen script.");
                            match try_compile(
                                to_tui,
                                from_watcher,
                                chosen_script,
                                chosen_script_path,
                                runtime,
                            ) {
                                Ok(_) => {
                                    // Script fixed.
                                    return Ok(());
                                }
                                Err(_) => {
                                    // Didn't work out try one more time.
                                    continue;
                                }
                            }
                        }
                    }
                    Err(_e) => {}
                }
            }
        }
    }
}

// Borrowed from,
// https://stackoverflow.com/questions/26958489/how-to-copy-a-folder-recursively-in-rust
pub fn copy_directory_contents<U: AsRef<Path>, V: AsRef<Path>>(
    from: U,
    to: V,
) -> Result<(), std::io::Error> {
    let mut stack = vec![PathBuf::from(from.as_ref())];
    let output_root = PathBuf::from(to.as_ref());
    let input_root = PathBuf::from(from.as_ref()).components().count();

    while let Some(working_path) = stack.pop() {
        println!("process: {:?}", &working_path);

        // Generate a relative path
        let src: PathBuf = working_path.components().skip(input_root).collect();

        // Create a destination if missing
        let dest = if src.components().count() == 0 {
            output_root.clone()
        } else {
            output_root.join(&src)
        };
        if fs::metadata(&dest).is_err() {
            println!(" mkdir: {:?}", dest);
            fs::create_dir_all(&dest)?;
        }

        std::thread::sleep(std::time::Duration::from_millis(1000));
        for entry in fs::read_dir(working_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                match path.file_name() {
                    Some(filename) => {
                        let dest_path = dest.join(filename);
                        println!("  copy: {:?} -> {:?}", &path, &dest_path);
                        fs::copy(&path, &dest_path)?;
                    }
                    None => {
                        println!("failed: {:?}", path);
                    }
                }
            }
        }
    }

    Ok(())
}
