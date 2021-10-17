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

enum Exit {
    Unavailable = 0x66,
    Success = 0x00,
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

    // Initialize terminal ui (tui)
    let tui = Arc::new(Mutex::new(Tui::new()));
    let (to_tui, from_main) = channel::<MainToTuiMessage>();
    let tui_clone = tui.clone();

    // Start the listener for terminal ui (tui)
    std::thread::spawn(move || loop {
        use MainToTuiMessage::*;
        match from_main.recv() {
            Ok(message) => match message {
                // Greetings
                Intro => {
                    lock!(tui_clone)
                        .intro()
                        .expect("Failed to write to stdout.");
                }
                // List available scripts
                ListScripts(index, file_name) => {
                    lock!(tui_clone)
                        .elements_to_choose(&index, &file_name)
                        .expect("Failed to write to stdout.");
                }
                // Instruct user to choose
                WaitForChoice => {
                    lock!(tui_clone)
                        .wait_for_choice()
                        .expect("Failed to write to stdout.");
                }
                // Ignore invalid choices
                IgnoreChoice => {
                    lock!(tui_clone)
                        .ignore_choice()
                        .expect("Failed to write to stdout.");
                }
                // Clear screen
                Clear => {
                    lock!(tui_clone)
                        .clear()
                        .expect("Failed to write to stdout.");
                }
                // Highlight choice and list scripts again
                HighlightAndRender(chosen_index, available_scripts) => {
                    lock!(tui_clone)
                        .highlight_and_render(&chosen_index, &available_scripts)
                        .expect("Failed to write to stdout.");
                }
                // Show which script is erroring
                ErrorInScript(info, err) => {
                    lock!(tui_clone)
                        .show_error(&info)
                        .expect("Failed to write to stdout.");
                    eprintln!("{}", err);
                }
            },
            Err(_e) => {
                // TODO: Anything to cover here?
            }
        }
    });

    // Try to discover user's home directory
    let home = match home_dir() {
        Some(dir) => dir,
        None => match matches.value_of("home") {
            Some(path) => PathBuf::from(path),
            None => {
                lock!(tui).no_home()?;
                std::process::exit(Exit::Unavailable as i32);
            }
        },
    };

    let scripts_folder_path = get_scripts_folder_path(
        home.to_str()
            .expect("Tried to convert invalid unicode string."),
    );
    let scripts_folder_path_str = scripts_folder_path
        .to_str()
        .expect("Tried to convert invalid unicode string.");

    if matches.is_present("clean") {
        fs::remove_dir_all(scripts_folder_path_str)?;
        lock!(tui).removed_scripts_folder()?;
        std::process::exit(Exit::Success as i32);
    }

    if matches.is_present("reset") {
        fs::remove_dir_all(scripts_folder_path_str)?;
        lock!(tui).reset_scripts_folder()?;

        let mut examples_path = PathBuf::new();
        examples_path.push(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(
            examples_path
                .to_str()
                .expect("Tried to convert invalid unicode string."),
            scripts_folder_path_str,
        )?;
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

    // List and collect all scripts which has a ".koto" extension.
    for (index, path) in script_paths.enumerate() {
        let path_buf = path?.path();
        match path_buf.extension() {
            Some(extension) => match extension.to_str() {
                Some(extension) => {
                    if "koto" == extension {
                        to_tui.send(MainToTuiMessage::ListScripts(
                            index.to_string(),
                            // This will always succeed.
                            format!("{:?}", path_buf.file_name().unwrap()),
                        ))?;
                        let full_path = format!("{}", path_buf.display());
                        available_scripts.push(full_path);
                    }
                }
                None => {
                    continue;
                }
            },
            None => {
                continue;
            }
        }
    }

    // "~/.mep" folder is empty
    if available_scripts.len() == 0 {
        lock!(tui).empty_scripts_folder()?;
        std::process::exit(Exit::Unavailable as i32);
    }

    // Start a watcher for "~/.mep" folder in its own thread.
    let (to_main, from_watcher) = channel::<WatcherToMainMessage>();
    std::thread::spawn(move || loop {
        let (sender, receiver) = channel();
        let mut watcher =
            watcher(sender, Duration::from_secs(1)).expect("Failed to create script watcher.");
        let mut watcher_path = home.clone();
        watcher_path.push(SCRIPTS_FOLDER_NAME);

        watcher
            .watch(
                watcher_path.to_str().expect("Failed to convert "),
                RecursiveMode::Recursive,
            )
            .expect("Watch of \"~/.mep\" folder failed.");

        // Blocks until receives
        match receiver.recv() {
            Ok(event) => match event {
                DebouncedEvent::Write(path) | DebouncedEvent::NoticeWrite(path) => {
                    if let Some(extension) = path.extension() {
                        match extension.to_str() {
                            Some(extension) if "koto" == extension => {
                                match to_main.send(WatcherToMainMessage::Change(path)) {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("{:?}", e);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            Err(err) => println!("{:?}", err),
        }
    });

    let mut choice = String::new();
    let mut chosen_idx: usize;
    let max_idx = available_scripts.len() - 1;
    to_tui.send(MainToTuiMessage::WaitForChoice)?;

    loop {
        // Get user input
        stdin().read_line(&mut choice)?;
        chosen_idx = match choice.trim().parse() {
            Ok(idx) => idx,
            Err(_) => {
                // User entered invalid value try again
                choice.clear();
                to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                continue;
            }
        };
        if chosen_idx > max_idx {
            // User entered value out of bounds try again
            choice.clear();
            to_tui.send(MainToTuiMessage::IgnoreChoice)?;
            continue;
        }
        break;
    }

    let chosen_script =
        fs::read_to_string(&available_scripts[chosen_idx]).expect("Couldn't read chosen script.");
    let chosen_script_path = available_scripts[chosen_idx].clone();

    // Init script runtime
    let shared_koto_runtime = Arc::new(Mutex::new(Koto::default()));
    let shared_koto_runtime_clone = shared_koto_runtime.clone();

    let mut runtime = lock!(shared_koto_runtime_clone);
    runtime.set_script_path(Some(PathBuf::from(&available_scripts[chosen_idx])));

    // Init midi ports
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

    // Init "koto_midi" library
    let mut midi_module = koto_midi::make_module();
    let send_error_message = "send requires a list of bytes [0 - 255], 
    you may still send malformed messages with this restriction. 
    There will be no problem if you obey the protocol ;)";

    // Add "midi.send" function
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
            let _result: Result<_, RuntimeError> =
                // This will always succeed.
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

    // Make the handler call "midi.listen" function
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
                                        let message_values = message.iter().map(|byte| Value::Number(byte.into())).collect::<Vec<Value>>();
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
            (),
        ).expect("Couldn't create a virtual input midi port.");

    // Add "koto_midi", "random" and other custom extensions to script runtime prelude.
    let mut prelude = runtime.prelude();
    prelude.add_map("midi", midi_module);
    prelude.add_value("random", koto_random::make_module());

    // Tries to compile the chosen script with dynamic error handling.
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

    runtime.run()?;

    // A thread for non-blocking stdin
    fn spawn_stdin_channel() -> Receiver<String> {
        let (stdin_to_main, from_stdin) = channel::<String>();
        std::thread::spawn(move || loop {
            let mut choice = String::new();
            match stdin().read_line(&mut choice) {
                Ok(_) => match stdin_to_main.send(choice) {
                    Ok(_) => {}
                    Err(e) => {
                        eprintln!("{:?}", e);
                    }
                },
                Err(e) => eprintln!("{:?}", e),
            }
        });
        from_stdin
    }
    let stdin_channel = spawn_stdin_channel();

    // Main loop
    loop {
        match stdin_channel.try_recv() {
            Ok(mut choice) => {
                chosen_idx = match choice.clone().trim().parse() {
                    Ok(idx) => idx,
                    Err(_) => {
                        // User entered invalid value try again
                        choice.clear();
                        to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                        continue;
                    }
                };

                // User entered value out of bounds try again
                if chosen_idx > max_idx {
                    choice.clear();
                    to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                    continue;
                }

                let chosen_script_path = available_scripts[chosen_idx].clone();
                let chosen_script =
                    fs::read_to_string(&chosen_script_path).expect("Couldn't read chosen script.");

                // Tries to compile the chosen script with dynamic error handling.
                try_compile(
                    &to_tui,
                    &from_watcher,
                    chosen_script,
                    chosen_script_path.clone(),
                    &mut runtime,
                )?;

                to_tui.send(MainToTuiMessage::HighlightAndRender(
                    chosen_idx.to_string(),
                    (*available_scripts).clone(),
                ))?;
            }
            Err(TryRecvError::Empty) => match from_watcher.try_recv() {
                // After checking for user input, check if a script is changed.
                Ok(message) => {
                    #[allow(irrefutable_let_patterns)]
                    if let WatcherToMainMessage::Change(path) = message {
                        loop {
                            let chosen_script_path = path
                                .to_str()
                                .expect("Tried to convert invalid unicode string.")
                                .to_string();
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
                                    // Script fixed or there was no problem.
                                    // Re-render
                                    to_tui.send(MainToTuiMessage::HighlightAndRender(
                                        chosen_idx.to_string(),
                                        (*available_scripts).clone(),
                                    ))?;
                                    break;
                                }
                                Err(_) => {
                                    // Script still has errors. Try one more time.
                                    continue;
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    // TODO: Anything to cover here?
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
                // Compile time error found in script.
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
                                // A fix attempt had been made.
                                let chosen_script_path = path
                                    .to_str()
                                    .expect("Tried to convert invalid unicode string.")
                                    .to_string();
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
                                        // Script is fixed.
                                        return Ok(());
                                    }
                                    Err(_) => {
                                        // Didn't work out try one more time.
                                        continue;
                                    }
                                }
                            }
                        }
                        Err(_e) => {
                            // TODO: Anything to cover here?
                        }
                    }
                }
            }
        },
        Err(e) => {
            // Compile time error found in script.
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
                            // A fix attempt had been made.
                            let chosen_script_path = path
                                .to_str()
                                .expect("Tried to convert invalid unicode string.")
                                .to_string();
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
                                    // Script is fixed.
                                    return Ok(());
                                }
                                Err(_) => {
                                    // Didn't work out try one more time.
                                    continue;
                                }
                            }
                        }
                    }
                    Err(_e) => {
                        // TODO: Anything to cover here?
                    }
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
                        // println!("  copy: {:?} -> {:?}", &path, &dest_path);
                        fs::copy(&path, &dest_path)?;
                    }
                    None => {
                        eprintln!("failed: {:?}", path);
                    }
                }
            }
        }
    }

    Ok(())
}
