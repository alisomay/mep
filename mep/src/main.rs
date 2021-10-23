#![warn(
    clippy::all,
    clippy::pedantic,
    clippy::restriction,
    clippy::nursery,
    clippy::cargo
)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]
#![allow(
    clippy::integer_arithmetic,
    clippy::float_arithmetic,
    clippy::blanket_clippy_restriction_lints,
    clippy::implicit_return,
    clippy::missing_docs_in_private_items,
    clippy::too_many_lines,
    clippy::enum_glob_use,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::missing_errors_doc,
    clippy::pattern_type_mismatch,
    // clippy::shadow_unrelated,
    clippy::shadow_reuse
)]
#![feature(stmt_expr_attributes)]

mod tui;
use dirs::home_dir;
use std::{fs, io::stdin, path::{Path, PathBuf}, sync::mpsc::channel, sync::{
        mpsc::{Receiver, TryRecvError},
        Arc, Mutex,
    }, time::Duration};
use tui::{Tui, BULB};

use koto::{
    runtime::{RuntimeError, RuntimeErrorType, Value, ValueList, ValueNumber},
    Koto
};
use midir::{
    os::unix::{VirtualInput, VirtualOutput},
    MidiInput, MidiOutput,
};

// TODO: Use and make use of Context
use anyhow::{anyhow, bail, Result};
use clap::{App, Arg, ArgMatches};
use crossterm::style::Stylize;

use notify::{watcher, DebouncedEvent, RecursiveMode, Watcher};

const SCRIPTS_FOLDER_NAME: &str = ".mep";
#[derive(Debug)]
enum WatcherToMainMessage {
    NoticeWrite(PathBuf),
    NoticeRemove(PathBuf),
    Create(PathBuf),
    Error(notify::Error,Option<PathBuf>),
}
#[derive(Debug)]
struct Context {
    scripts_folder_path: PathBuf,
    available_scripts: Vec<String>,
    chosen_index_checked: usize,
    chosen_script:String,
    chosen_script_path:String
}
impl From<(PathBuf,Vec<String>,usize,String,String)> for Context {
    fn from(members:(PathBuf,Vec<String>,usize,String,String)) -> Self {
        Self {
            scripts_folder_path: members.0,
            available_scripts: members.1,
            chosen_index_checked: members.2,
            chosen_script: members.3,
            chosen_script_path: members.4
        }
    }
}

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

    let tui = Tui::new();

    // Try to discover user's home directory
    let home = match home_dir() {
        Some(dir) => dir,
        None => {
            if let Some(path) = matches.value_of("home") {
                PathBuf::from(path)
            } else {
                tui.clear_lines(1)?;
                bail!("{} {}", BULB, "\"mep\" couldn't determine the location of your home directory, to help it please run it with \"--home <absolute-path-to-your-home-directory>\"".blue());
            }
        }
    };

    let scripts_folder_path = get_scripts_folder_path(&home.to_string_lossy());

    if matches.is_present("clean") {
        fs::remove_dir_all(scripts_folder_path)?;
        tui.removed_scripts_folder()?;
        // Exit successfully
        return Ok(());
    }

    if matches.is_present("reset") {
        fs::remove_dir_all(&scripts_folder_path)?;
        tui.reset_scripts_folder()?;

        let mut examples_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(examples_path, &scripts_folder_path)?;
    }

    if !scripts_folder_path.exists() {
        tui.scripts_folder_not_found()?;

        let mut examples_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        examples_path.push("..");
        examples_path.push("example_scripts");

        copy_directory_contents(
            &format!("{}", examples_path.display()),
            &scripts_folder_path,
        )?;
    }


    let mut available_scripts = vec![];
    collect_available_scripts_to(&mut available_scripts,&scripts_folder_path)?;

    tui.clear()?;
    tui.intro()?;

    // "~/.mep" folder is empty
    if available_scripts.is_empty() {
        tui.clear_lines(1)?;
        bail!(
            "{} {}",
            BULB,
            "There are no event processor scripts found in \"~/.mep\". Maybe put a couple?".blue()
        );
    }

    tui.list_scripts(&available_scripts)?;

    // Start a watcher for "~/.mep" folder in its own thread.
    let (to_main, from_watcher) = channel::<WatcherToMainMessage>();
    let _watcher_thread = std::thread::spawn(move || -> Result<()> {
        fn is_koto_script(path: &Path) -> Result<()> {
            // Is meaningful errors needed here?
            path.extension().map_or_else(|| Err(anyhow!("File does not have any extension.")), |extension| match extension.to_str() {
                    Some(extension) if "koto" == extension => {
                       Ok(())
                    }                    
                    None | Some(_) => Err(anyhow!("File does not have a \".koto\" extension.")) 
            })
        }

        loop {
            let (sender, receiver) = channel();
            let mut watcher = watcher(sender, Duration::from_millis(100))?;
            let mut watcher_path = home.clone();
            watcher_path.push(SCRIPTS_FOLDER_NAME);
            watcher.watch(watcher_path, RecursiveMode::Recursive)?;

            if let Ok(event) = receiver.recv() {
                match event {
                    DebouncedEvent::NoticeWrite(path) => {
                        if is_koto_script(&path).is_ok() {
                            to_main.send(WatcherToMainMessage::NoticeWrite(path))?;
                        }
                    }
                    DebouncedEvent::NoticeRemove(path) => {
                        if is_koto_script(&path).is_ok() {
                            to_main.send(WatcherToMainMessage::NoticeRemove(path))?;
                        }
                    }
                    DebouncedEvent::Create(path) => {    
                        if is_koto_script(&path).is_ok() {
                            to_main.send(WatcherToMainMessage::Create(path))?;
                        }
                    }
                    DebouncedEvent::Error(err, path) => {
                        to_main.send(WatcherToMainMessage::Error(err, path))?;
                    },
                    DebouncedEvent::Write(_) | DebouncedEvent::Remove(_) | DebouncedEvent::Chmod(_) | DebouncedEvent::Rescan | DebouncedEvent::Rename(_,_) => {
                        // Currently ignoring these.
                        // Rename event is actually a real mv.
                    }
                }
            }
        }
    });

    let mut choice = String::new();
    let mut chosen_index_checked: usize;
    // This operation is sound because, at this point we know that "available_scripts" is greater than 0.
    let max_idx = available_scripts.len() - 1;

    loop {
        // Get user input
        stdin().read_line(&mut choice)?;
        chosen_index_checked = if let Ok(idx) = choice.trim().parse() {
            idx
        } else {
            // User entered invalid value or negative value, try again
            choice.clear();
            tui.ignore_choice()?;
            continue;
        };
        if chosen_index_checked > max_idx {
            // User entered index out of positive bounds, try again
            choice.clear();
            tui.ignore_choice()?;
            continue;
        }
        break;
    }

    let chosen_script = fs::read_to_string(&available_scripts[chosen_index_checked])?;
    let chosen_script_path = available_scripts[chosen_index_checked].clone();

    // Init script runtime
    let mut runtime = Koto::default();
    runtime.set_script_path(Some(PathBuf::from(
        &available_scripts[chosen_index_checked],
    )));


    let (mep_in, mep_out, input_port_name, output_port_name) = init_midi_io(&matches)?;

    let mep_out_port = Arc::new(Mutex::new(
        mep_out.create_virtual(&output_port_name).map_err(|err| {
            anyhow!(
                "Couldn't create virtual midi output port named {}.\nError: {:?}",
                output_port_name,
                err
            )
        })?,
    ));

    // Init "koto_midi" library
    let mut midi_module = koto_midi::make_module();
    let send_error_message = "Error calling \"midi.send\": Wrong argument type, please try to use a list of bytes (integers ranged to 0..=255) as an argument. Ex. [144, 65, 127]";

    
    // Add "midi.send" function
    let (midi_send_error_to_main, midi_send_errors) = std::sync::mpsc::sync_channel(256);
    midi_module.add_fn("send", move |vm, args| if let [Value::List(message)] = vm.get_args(args) {
        let msg: Result<Vec<u8>,_> = message
            .data()
            .iter()
            .map(|value| if let Value::Number(num) = *value { match num {
                #[allow(clippy::cast_sign_loss)]
                #[allow(clippy::cast_possible_truncation)]
                #[allow(clippy::as_conversions)]
                // These are all fine because the value of `byte` is checked if it is in u8 range before.
                ValueNumber::I64(byte) if (0..=255).contains(&byte) => Ok(byte as u8),
                _ => {
                    // `unwrap()` will always succeed channel receiver is in main.
                    #[allow(clippy::unwrap_used)]
                    midi_send_error_to_main.send(send_error_message.into()).unwrap();
                    Err(())
                }
            } } else {
                // `unwrap()` will always succeed succeed channel receiver is in main.
                #[allow(clippy::unwrap_used)]
                midi_send_error_to_main.send(send_error_message.into()).unwrap();
                Err(()) 
            }).collect();
            if let Ok(midi_message_to_send) = msg {
                // `lock.unwrap()` will always succeed no one else locks it.
                #[allow(clippy::unwrap_used)]
                if let Err(e) = mep_out_port.lock().unwrap().send(&midi_message_to_send[..]) {
                    // `unwrap()` will always succeed succeed channel receiver is in main.
                    midi_send_error_to_main.send(format!("Error when trying to send midi message: {}", e)).unwrap();
                }
            }
        Ok(Value::Empty)
    } else {
        midi_send_error_to_main.send(send_error_message.to_owned())
        .map(|_| Value::Empty)
        .map_err(|err| RuntimeError::from(err.to_string()))
    });

    // Make the handler call "midi.listen" function
    let (midi_in_to_main, from_midi_in) = channel::<Vec<u8>>();
    let _mep_in_port = mep_in
        .create_virtual(
            &input_port_name,
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
                input_port_name,
                err
            )
        })?;

    // Add "koto_midi", "random" and other custom extensions to script runtime prelude.
    let mut prelude = runtime.prelude();
    prelude.add_map("midi", midi_module);
    prelude.add_value("random", koto_random::make_module());


    let mut context = Context::from((scripts_folder_path,available_scripts,chosen_index_checked,chosen_script,chosen_script_path));


    // Tries to compile the chosen script with dynamic error handling.
    compile_run_block_until_valid(
        &tui,
        &from_watcher,
        &mut context,
        &mut runtime,
    )?;

    tui.highlight_and_render(&chosen_index_checked.to_string(), &context.available_scripts)?;

    runtime.run()?;

    // A receiver for the thread for non-blocking stdin
    let stdin_channel = spawn_stdin_channel();

    // Main loop
    loop {
        // TODO: Might be better to move this midi receiver to it's own thread. And propagate necessary messages there..
        // The problem is runtime creates a deadlock if we move the clone of it inside the closure..
        // Find a nice solution to it.
        // If we don't throttle this loop it consumes %100 CPU as expected.
        // For now, 0.00025 secs or low is enough for midi messages, average of %3.5~ CPU and 1ms round trip latency.
        // This technique of hot reloading in midi receive errors has drawbacks listed in the upper part.
        std::thread::sleep(std::time::Duration::from_micros(250));

        // Process midi received messages
        if let Ok(message) = from_midi_in.try_recv() {    
            match call_midi_listen_with(&message, &mut runtime) {
                Ok(_) => continue,
                Err(err) => {
                    tui.clear()?;
                    // TODO: maybe downcast ref here
                    if let RuntimeErrorType::StringError(error_message) = err.error {
                        tui.show_error(&context.chosen_script_path, &error_message)?;
                    }
                    if try_debug(&tui,&from_watcher, &mut runtime, &mut context).is_ok() {
                        continue;
                    };
                }
            }
        }
        if let Ok(error_message) = midi_send_errors.try_recv() {        
            tui.clear()?;
            tui.show_error(&context.chosen_script_path, &error_message)?;                        
            if try_debug(&tui,&from_watcher, &mut runtime, &mut context).is_ok() {
                continue;
            }
        }

        match stdin_channel.try_recv() {
            Ok(mut user_choice) => {        
                context.chosen_index_checked = if let Ok(idx) = user_choice.trim().parse() {
                    idx
                } else {
                    // User entered invalid value or negative value, try again
                    user_choice.clear();
                    tui.ignore_choice()?;
                    continue;
                };
                if chosen_index_checked > max_idx {
                    // User entered index out of positive bounds, try again
                    user_choice.clear();
                    tui.ignore_choice()?;
                    continue;
                }
                
                context.chosen_script_path = context.available_scripts[context.chosen_index_checked].clone();
                context.chosen_script = fs::read_to_string(&context.chosen_script_path)?;

                // Tries to compile the chosen script with dynamic error handling.
                compile_run_block_until_valid(
                    &tui,
                    &from_watcher,
                    &mut context,
                    &mut runtime,
                )?;

                tui.highlight_and_render(&context.chosen_index_checked.to_string(), &context.available_scripts)?;

            }
            Err(TryRecvError::Empty) => {
                if try_debug(&tui,&from_watcher, &mut runtime, &mut context).is_ok() {
                    continue;
                }  
            }
            // TODO: Maybe join the thread? Currently erroring and terminating.
            Err(TryRecvError::Disconnected) => bail!("stdin channel disconnected!"),
        }
    }

    // unreachable
}


fn collect_available_scripts_to(vector_to_collect_to: &mut Vec<String>, scripts_folder_path: &Path) -> Result<()> {
    let script_paths = fs::read_dir(&scripts_folder_path)?;
    // List and collect all scripts which has a ".koto" extension.
    for path in script_paths {
        let path_buf = path?.path();
        match path_buf.extension() {
            Some(extension) => match extension.to_str() {
                Some(extension) => {
                    if "koto" == extension {
                        let full_path = format!("{}", path_buf.display());
                        vector_to_collect_to.push(full_path);
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
    };
    Ok(())
}

fn get_scripts_folder_path(home: &str) -> PathBuf {
    let mut scripts_folder_path = PathBuf::new();
    scripts_folder_path.push(&home);
    scripts_folder_path.push(SCRIPTS_FOLDER_NAME);
    scripts_folder_path
}

fn call_midi_listen_with(message: &[u8], runtime: &mut Koto) -> Result<(), RuntimeError> {
    match runtime.prelude().data().get_with_string("midi") {
        Some(midi_module) => match midi_module {
            Value::Map(midi_module) => match midi_module.data().get_with_string("listen") {
                Some(message_listener) => match message_listener {
                    Value::Function(_) => {
                        // Make a list of koto values from u8 slice.
                        let message_values = message
                            .iter()
                            .map(|byte| Value::Number(byte.into()))
                            .collect::<Vec<Value>>();
                        // Call "midi.listen" function in script with the midi message.
                        runtime
                            .call_function(
                                message_listener.clone(),
                                &[Value::List(ValueList::from_slice(&message_values))],
                            )
                            .map(|_| ())
                            .map_err(|err|  
                                 RuntimeError::with_prefix(RuntimeError::from(format!("Calling \"midi.listen\" is failed, {}",err.to_string())),&"Error".magenta().to_string())
                            )
                    }
                    _ => Err(RuntimeError::with_prefix(
                        RuntimeError::from("\"midi.listen\" is defined but it is not a function".to_owned()),
                        &"Error".magenta().to_string(),
                    )),
                },
                None => Err(RuntimeError::with_prefix(
                    RuntimeError::from("Try defining a function as \"midi.listen\". If not there please try importing \"midi\" on top of your script like \"import midi\".".to_owned()),
                    &"Error".magenta().to_string(),
                )),
            },
            _ => Err(RuntimeError::with_prefix(
                RuntimeError::from("\"midi\" has been found but it is not a map. Try importing \"midi\" on top of your script like \"import midi\". And do not use the same name further.".to_owned()),
                &"Error".magenta().to_string(),
            )),
        },
        None => Err(RuntimeError::with_prefix(
            RuntimeError::from("Try importing \"midi\" on top of your script like \"import midi\"".to_owned()),
            &"Error".magenta().to_string(),
        )),
    }
}

fn init_midi_io(
    command_line_options: &ArgMatches,
) -> Result<(MidiInput, MidiOutput, String, String)> {
    let mep_in = MidiInput::new("mep_input")?;
    let mep_out = MidiOutput::new("mep_output")?;

    let mut input_port_name = String::from("_in");
    let mut output_port_name = String::from("_out");

    let mep_input_port_name = match command_line_options.value_of("port") {
        Some(port_name) => {
            input_port_name.insert_str(0, port_name);
            input_port_name
        }
        None => "mep_in".to_owned(),
    };
    let mep_output_port_name = match command_line_options.value_of("port") {
        Some(port_name) => {
            output_port_name.insert_str(0, port_name);
            output_port_name
        }
        None => "mep_out".to_owned(),
    };
    Ok((mep_in, mep_out, mep_input_port_name, mep_output_port_name))
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


fn try_debug(tui: &Tui, watcher_channel:&Receiver<WatcherToMainMessage>, runtime: &mut Koto, context: &mut Context) -> Result<()> {
    if let Ok(message_from_watcher) = watcher_channel.try_recv() {
        use WatcherToMainMessage::*;
        match message_from_watcher {
            NoticeWrite(path) =>  {
                // We need to make this path shared.
                context.chosen_script_path = path.to_string_lossy().into();
                context.chosen_script = fs::read_to_string(&context.chosen_script_path)?;
                if compile_run_block_until_valid(
                    tui,
                    watcher_channel,
                    context,
                    runtime,
                ).is_ok() {   
                    // Script fixed or there was no problem.
                    tui.highlight_and_render(
                    &context.chosen_index_checked.to_string(),
                    &context.available_scripts,
                    )?;
                    return Ok(());
                }
            }
            NoticeRemove(_) => {
                // Get diff
                let mut new_available_scripts = vec![];
                collect_available_scripts_to(&mut new_available_scripts, &context.scripts_folder_path)?;
                let modified_script_path: String = new_available_scripts.clone().into_iter().filter(|item| !context.available_scripts.contains(item)).collect();
                // dbg!(&context.chosen_script_path,&path,modified_script_path);
                // Replace available scripts.
                std::mem::swap(&mut context.available_scripts,&mut new_available_scripts);
                
                if fs::read_to_string(&context.chosen_script_path).is_err() { 
                    // dbg!(&context.chosen_script_path,&path);
                    // std::thread::sleep(Duration::from_secs(4));
                    // Either the currently chosen script is removed or renamed.
                    if fs::read_to_string(&modified_script_path).is_ok() {
                        // Script is renamed
                        context.chosen_script = fs::read_to_string(&modified_script_path)?;
                        context.chosen_script_path = modified_script_path;
                        // Update chosen index.
                        for (i, path) in context.available_scripts.iter().enumerate() {
                            if *path == context.chosen_script_path {
                                context.chosen_index_checked = i;
                            }
                        }   
                    }
                    else {
                        // Script is removed, check if there are available scripts.
                        // "~/.mep" folder is empty
                        if context.available_scripts.is_empty() {
                            tui.clear_lines(1)?;
                            bail!(
                                "{} {}",
                                BULB,
                                "There are no event processor scripts found in \"~/.mep\". Maybe put a couple?".blue()
                            );
                        }
                        
                        // Fall back to first script in the list
                        context.chosen_script = fs::read_to_string(&context.available_scripts[0])?;
                        context.chosen_script_path = context.available_scripts[0].clone();
                        context.chosen_index_checked = 0;
                    }
                     // Run new script
                    if compile_run_block_until_valid(
                        tui,
                        watcher_channel,
                        context,
                        runtime,
                    ).is_ok() {   
                        // Script fixed or there was no problem.
                        tui.highlight_and_render(
                        &context.chosen_index_checked.to_string(),
                        &context.available_scripts,
                        )?;    
                        return Ok(());
                    }
                }
                else {
                    // Another script is removed.
                    // Just re-render.
    
                    tui.highlight_and_render(
                    &context.chosen_index_checked.to_string(),
                    &context.available_scripts,
                    )?;
                    return Ok(());
                }
            },
            Create(_) => {
                // Just re-list the scripts with the existing choice.
                let mut new_available_scripts = vec![];
                collect_available_scripts_to(&mut new_available_scripts, &context.scripts_folder_path)?;
                std::mem::swap(&mut context.available_scripts,&mut new_available_scripts);

                tui.highlight_and_render(
                &context.chosen_index_checked.to_string(),
                &context.available_scripts,
                )?;
                return Ok(());
            },
            Error(err,path) => {
                let p: String = match path {
                    Some (path) => path.to_string_lossy().into(),
                    None => "".into()
                };
                tui.clear_lines(1)?;
                bail!(
                    "{} {} with the script located in {}. Message: {}",
                    BULB,
                    "Error ".magenta(),
                    p,
                    err.to_string(),
                );
            }
        }
    }
    Err(anyhow!("No change in \"~/.mep\" folder detected."))

}

fn compile_run_block_until_valid(
    tui: &Tui,
    from_watcher: &Receiver<WatcherToMainMessage>,
    context: &mut Context,
    runtime: &mut Koto,
) -> Result<()> {
    match runtime.compile(&context.chosen_script) {
        Ok(chunk) => match runtime.run_chunk(chunk) {
            Ok(_) => {
                Ok(())
            },
            Err(err) => {
                // Runtime time error found in script.
                tui.clear()?;
                tui.show_error(&context.chosen_script_path, &err.to_string())?;
                loop {
                    if let Ok(WatcherToMainMessage::NoticeWrite(path)) = from_watcher.recv() {
                        // A fix attempt had been made.
                        context.chosen_script_path = path.to_string_lossy().into();
                        context.chosen_script = fs::read_to_string(&context.chosen_script_path)?;
                        match compile_run_block_until_valid(
                            tui,
                            from_watcher,
                            context,
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
        Err(err) => {
            // Compile time error found in script.
            tui.clear()?;
            tui.show_error(&context.chosen_script_path, &err.to_string())?;
            loop {
                if let Ok(WatcherToMainMessage::NoticeWrite(path)) = from_watcher.recv() {
                    // A fix attempt had been made.
                    context.chosen_script_path = path.to_string_lossy().into();
                    context.chosen_script = fs::read_to_string(&context.chosen_script_path)?;
                    match compile_run_block_until_valid(
                        tui,
                        from_watcher,
                        context,
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
                        return Err(anyhow!("failed: {:?}", path));
                    }
                }
            }
        }
    }
    Ok(())
}
