#![warn(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    clippy::cargo
)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]
#![allow(
    clippy::blanket_clippy_restriction_lints,
    clippy::implicit_return,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::enum_glob_use,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::missing_errors_doc,
    clippy::pattern_type_mismatch
)]
#![feature(stmt_expr_attributes)]

mod tui;
use dirs::home_dir;
use std::{
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
use tui::{Tui, BULB};

use koto::{
    runtime::{runtime_error, RuntimeError, Value, ValueList, ValueNumber},
    Koto,
};
use midir::{
    os::unix::{VirtualInput, VirtualOutput},
    MidiInput, MidiOutput,
};

// TODO: Use and make use of Context
use anyhow::{anyhow, bail, Result};
use clap::{App, Arg};
use crossterm::style::Stylize;

use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};

const SCRIPTS_FOLDER_NAME: &str = ".mep";

macro_rules! lock {
    ($i:ident) => {
        $i.lock().unwrap()
    };
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

// TODO: Either determine the right error type for main or leave a trait object.
fn main() -> Result<()> {
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
    let tui_clone = Arc::clone(&tui);

    // Start the listener for terminal ui (tui)
    let _tui_thread = std::thread::spawn(move || -> Result<()> {
        loop {
            use MainToTuiMessage::*;
            if let Ok(message) = from_main.recv() {
                match message {
                    // Greetings
                    Intro => {
                        lock!(tui_clone).intro()?;
                    }
                    // List available scripts
                    ListScripts(index, file_name) => {
                        lock!(tui_clone).elements_to_choose(&index, &file_name)?;
                    }
                    // Instruct user to choose
                    WaitForChoice => {
                        lock!(tui_clone).wait_for_choice()?;
                    }
                    // Ignore invalid choices
                    IgnoreChoice => {
                        lock!(tui_clone).ignore_choice()?;
                    }
                    // Clear screen
                    Clear => lock!(tui_clone).clear()?,
                    // Highlight choice and list scripts again
                    HighlightAndRender(chosen_index, available_scripts) => {
                        lock!(tui_clone).highlight_and_render(&chosen_index, &available_scripts)?;
                    }
                    // Show which script is erroring
                    ErrorInScript(path_to_script, err) => {
                        lock!(tui_clone).show_error(&path_to_script, &err)?;
                    }
                }
            }
        }
    });

    // Try to discover user's home directory
    let home = match home_dir() {
        Some(dir) => dir,
        None => {
            if let Some(path) = matches.value_of("home") {
                PathBuf::from(path)
            } else {
                lock!(tui).clear_lines(1)?;
                bail!("{} {}", BULB, "\"mep\" couldn't determine the location of your home directory, to help it please run it with \"--home <absolute-path-to-your-home-directory>\"".blue());
            }
        }
    };

    let scripts_folder_path = get_scripts_folder_path(&home.to_string_lossy());

    if matches.is_present("clean") {
        fs::remove_dir_all(scripts_folder_path)?;
        lock!(tui).removed_scripts_folder()?;
        // Exit successfully
        return Ok(());
    }

    if matches.is_present("reset") {
        fs::remove_dir_all(&scripts_folder_path)?;
        lock!(tui).reset_scripts_folder()?;

        let mut examples_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(examples_path, &scripts_folder_path)?;
    }

    if !scripts_folder_path.exists() {
        lock!(tui).scripts_folder_not_found()?;

        let mut examples_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(
            &format!("{}", examples_path.display()),
            &scripts_folder_path,
        )?;
    }

    let script_paths = fs::read_dir(&scripts_folder_path)?;
    let mut available_scripts = vec![];

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
                            // This unwrap will always succeed because of the previous checks.
                            #[allow(clippy::unwrap_used)]
                            path_buf.file_name().unwrap().to_string_lossy().into(),
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
    if available_scripts.is_empty() {
        lock!(tui).clear_lines(1)?;
        bail!(
            "{} {}",
            BULB,
            "There are no event processor scripts found in \"~/.mep\". Maybe put a couple?".blue()
        );
    }

    // Start a watcher for "~/.mep" folder in its own thread.
    let (to_main, from_watcher) = channel::<WatcherToMainMessage>();
    let _watcher_thread = std::thread::spawn(move || -> Result<()> {
        loop {
            let (sender, receiver) = channel();
            let mut watcher = watcher(sender, Duration::from_secs(1))?;
            let mut watcher_path = home.clone();
            watcher_path.push(SCRIPTS_FOLDER_NAME);
            watcher.watch(watcher_path, RecursiveMode::Recursive)?;

            if let Ok(event) = receiver.recv() {
                match event {
                    // TODO: Sort out which events to respond
                    DebouncedEvent::Write(path) | DebouncedEvent::NoticeWrite(path) => {
                        if let Some(extension) = path.extension() {
                            match extension.to_str() {
                                Some(extension) if "koto" == extension => {
                                    // It is safe to unwrap here in my opinion because the receiver is the main thread.
                                    to_main.send(WatcherToMainMessage::Change(path))?;
                                }
                                _ => {
                                    // Ignore files other than koto scripts
                                }
                            }
                        }
                    }
                    DebouncedEvent::NoticeRemove(_)
                    | DebouncedEvent::Create(_)
                    | DebouncedEvent::Chmod(_)
                    | DebouncedEvent::Remove(_)
                    | DebouncedEvent::Rename(..)
                    | DebouncedEvent::Rescan
                    | DebouncedEvent::Error(..) => {
                        // Ignore other events
                    }
                }
            }
        }
    });

    let mut choice = String::new();
    let mut chosen_idx: usize;
    // At this point we know that "available_scripts" is greater than 0.
    #[allow(clippy::integer_arithmetic)]
    let max_idx = available_scripts.len() - 1;
    to_tui.send(MainToTuiMessage::WaitForChoice)?;

    loop {
        // Get user input
        stdin().read_line(&mut choice)?;
        chosen_idx = if let Ok(idx) = choice.trim().parse() {
            idx
        } else {
            // User entered invalid value or negative value, try again
            choice.clear();
            to_tui.send(MainToTuiMessage::IgnoreChoice)?;
            continue;
        };
        if chosen_idx > max_idx {
            // User entered index out of positive bounds, try again
            choice.clear();
            to_tui.send(MainToTuiMessage::IgnoreChoice)?;
            continue;
        }
        break;
    }

    let chosen_script = fs::read_to_string(&available_scripts[chosen_idx])?;
    let chosen_script_path = available_scripts[chosen_idx].clone();

    // Init script runtime
    let mut runtime = Koto::default();
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

    let mep_out_port = Arc::new(Mutex::new(
        mep_out
            .create_virtual(mep_output_port_name)
            .map_err(|err| {
                anyhow!(
                    "Couldn't create virtual midi output port named {}.\nError: {:?}",
                    mep_output_port_name,
                    err
                )
            })?,
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
                .map(|value| match *value {
                    Value::Number(num) => match num {
                        #[allow(clippy::cast_sign_loss)]
                        #[allow(clippy::cast_possible_truncation)]
                        #[allow(clippy::as_conversions)]
                        // These are all fine because the value of `byte` is checked if it is in u8 range before.
                        ValueNumber::I64(byte) if (0..=255).contains(&byte) => Ok(byte as u8),

                        _ => runtime_error!(send_error_message),
                    },
                    _ => {
                        runtime_error!(send_error_message)
                    }
                })
                .collect::<Result<Vec<u8>, RuntimeError>>();
            let _result: Result<_, RuntimeError> =
                // `&msg.unwrap()` will always succeed.
                #[allow(clippy::unwrap_used)]
                match lock!(mep_out_port).send(&msg.unwrap()[..]) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        runtime_error!(format!("Error when trying to send midi message: {}", e))
                    }
                };
            Ok(Value::Empty)
        }
        _ => runtime_error!(send_error_message),
    });

    // Make the handler call "midi.listen" function
    let (midi_in_to_main, from_midi_in) = channel::<Vec<u8>>();
    let _mep_in_port = mep_in
        .create_virtual(
            mep_input_port_name,
            move |_stamp, message, _| {
                let msg: Vec<u8> = message.iter().copied().collect();
                #[allow(clippy::unwrap_used)]
                // The receiver is in the main thread and will live through the whole lifetime of the app.
                // Because of this unwrap is safe here.
                midi_in_to_main.send(msg).unwrap();
            },
            (),
        )
        .map_err(|err| {
            anyhow!(
                "Couldn't create virtual midi input port named {}.\nError: {:?}",
                mep_input_port_name,
                err
            )
        })?;

    // Add "koto_midi", "random" and other custom extensions to script runtime prelude.
    let mut prelude = runtime.prelude();
    prelude.add_map("midi", midi_module);
    prelude.add_value("random", koto_random::make_module());

    // Tries to compile the chosen script with dynamic error handling.
    try_compile(
        &to_tui,
        &from_watcher,
        &chosen_script,
        chosen_script_path.clone(),
        &mut runtime,
    )?;

    to_tui.send(MainToTuiMessage::HighlightAndRender(
        chosen_idx.to_string(),
        available_scripts.clone(),
    ))?;

    runtime.run()?;

    // A thread for non-blocking stdin
    let stdin_channel = spawn_stdin_channel();

    // Main loop
    loop {
        // Process midi received messages
        if let Ok(message) = from_midi_in.try_recv() {
            loop {
                match call_midi_listen_with(&message, &mut runtime) {
                    Ok(_) => break,
                    Err(err) => {
                        to_tui.send(MainToTuiMessage::Clear)?;
                        to_tui.send(MainToTuiMessage::ErrorInScript(
                            chosen_script_path.clone(),
                            err.to_string(),
                        ))?;
                        if let Ok(WatcherToMainMessage::Change(_)) = from_watcher.recv() {
                            // A fix attempt had been made.
                            if call_midi_listen_with(&message, &mut runtime).is_ok() {
                                // Script is fixed.                                                                        // Re-render
                                to_tui.send(MainToTuiMessage::HighlightAndRender(
                                    chosen_idx.to_string(),
                                    available_scripts.clone(),
                                ))?;
                                break;
                            } 
                            
                            // Try one more time
                            continue;
                            
                        }
                    }
                }
            }
        }

        match stdin_channel.try_recv() {
            Ok(mut choice) => {
                chosen_idx = if let Ok(idx) = choice.trim().parse() {
                    idx
                } else {
                    // User entered invalid value or negative value, try again
                    choice.clear();
                    to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                    continue;
                };
                if chosen_idx > max_idx {
                    // User entered index out of positive bounds, try again
                    choice.clear();
                    to_tui.send(MainToTuiMessage::IgnoreChoice)?;
                    continue;
                }

                let chosen_script_path = available_scripts[chosen_idx].clone();
                let chosen_script = fs::read_to_string(&chosen_script_path)?;

                // Tries to compile the chosen script with dynamic error handling.
                try_compile(
                    &to_tui,
                    &from_watcher,
                    &chosen_script,
                    chosen_script_path.clone(),
                    &mut runtime,
                )?;

                to_tui.send(MainToTuiMessage::HighlightAndRender(
                    chosen_idx.to_string(),
                    available_scripts.clone(),
                ))?;
            }
            Err(TryRecvError::Empty) => {
                if let Ok(WatcherToMainMessage::Change(path)) = from_watcher.try_recv() {
                    loop {
                        let chosen_script_path = path.to_string_lossy().into();
                        let chosen_script = fs::read_to_string(&chosen_script_path)?;
                        match try_compile(
                            &to_tui,
                            &from_watcher,
                            &chosen_script,
                            chosen_script_path,
                            &mut runtime,
                        ) {
                            Ok(_) => {
                                // Script fixed or there was no problem.
                                // Re-render
                                to_tui.send(MainToTuiMessage::HighlightAndRender(
                                    chosen_idx.to_string(),
                                    available_scripts.clone(),
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
            // TODO: Maybe join the thread? Currently erroring and terminating.
            Err(TryRecvError::Disconnected) => bail!("stdin channel disconnected!"),
        }
    }
}

fn call_midi_listen_with(message: &[u8], runtime: &mut Koto) -> Result<()> {
    match runtime.prelude().data().get_with_string("midi") {
        Some(midi_module) => match midi_module {
            Value::Map(midi_module) => match midi_module.data().get_with_string("listen") {
                Some(message_listener) => match message_listener {
                    Value::Function(_) => {
                        // Make a list of koto values from u8 slice.
                        let message_values = message.iter().map(|byte| Value::Number(byte.into())).collect::<Vec<Value>>();
                        // Call "midi.listen" function in script with the midi message.
                        runtime.call_function(
                            message_listener.clone(),
                            &[Value::List(ValueList::from_slice(&message_values))],
                        ).map(|_| ()).map_err(|err| anyhow!(err.to_string()))                          
                    }
                    _ => Err(anyhow!("\"midi.listen\" is defined but it is not a function")),
                },
                None => {
                    Err(anyhow!("Try defining a function as \"midi.listen\""))
                }
            },
            _ => Err(anyhow!("\"midi\" has been found but it is not a map. Try importing \"midi\" on top of your script like \"import midi\". And do not use the same name further.")),
        },
        None => Err(anyhow!("Try importing \"midi\" on top of your script like \"import midi\"")),
    }
}


fn spawn_stdin_channel() -> Receiver<String> {
    let (stdin_to_main, from_stdin) = channel::<String>();
    std::thread::spawn(move || -> Result<Receiver<String>> {
        loop {
            let mut choice = String::new();
            if stdin().read_line(&mut choice).is_ok() {
                stdin_to_main.send(choice)?;
            }
        }
    });
    from_stdin
}
fn try_compile(
    to_tui: &std::sync::mpsc::Sender<MainToTuiMessage>,
    from_watcher: &Receiver<WatcherToMainMessage>,
    chosen_script: &str,
    chosen_script_path: String,
    runtime: &mut Koto,
) -> Result<()> {
    match runtime.compile(chosen_script) {
        Ok(chunk) => match runtime.run_chunk(chunk) {
            Ok(_) => Ok(()),
            Err(e) => {
                // Runtime time error found in script.
                to_tui.send(MainToTuiMessage::Clear)?;
                to_tui.send(MainToTuiMessage::ErrorInScript(
                    chosen_script_path,
                    format!("{:?}", e),
                ))?;
                loop {
                    if let Ok(WatcherToMainMessage::Change(path)) = from_watcher.recv() {
                        // A fix attempt had been made.
                        let chosen_script_path = path.to_string_lossy().into();
                        let chosen_script = fs::read_to_string(&chosen_script_path)?;
                        match try_compile(
                            to_tui,
                            from_watcher,
                            &chosen_script,
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
                if let Ok(WatcherToMainMessage::Change(path)) = from_watcher.recv() {
                    // A fix attempt had been made.
                    let chosen_script_path = path.to_string_lossy().into();
                    let chosen_script = fs::read_to_string(&chosen_script_path)?;
                    match try_compile(
                        to_tui,
                        from_watcher,
                        &chosen_script,
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
        }
    }
}

fn get_scripts_folder_path(home: &str) -> PathBuf {
    let mut scripts_folder_path = PathBuf::new();
    scripts_folder_path.push(&home);
    scripts_folder_path.push(SCRIPTS_FOLDER_NAME);
    scripts_folder_path
}

// Borrowed from,
// https://stackoverflow.com/questions/26958489/how-to-copy-a-folder-recursively-in-rust
pub fn copy_directory_contents<U: AsRef<Path>, V: AsRef<Path>>(from: U, to: V) -> Result<()> {
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
                        // TODO: Add it to MepError maybe?
                        return Err(anyhow!("failed: {:?}", path));
                    }
                }
            }
        }
    }

    Ok(())
}
