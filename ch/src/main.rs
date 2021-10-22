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
    clippy::pattern_type_mismatch,
    clippy::shadow_unrelated,
    clippy::shadow_reuse
)]
#![feature(stmt_expr_attributes)]

use std::{
    fs,
    io::stdin,
    path::{Path, PathBuf},
    sync::mpsc::channel,
    sync::{
        mpsc::{Receiver, TryRecvError},
        Arc, Mutex,
    },
    thread::{sleep, spawn},
    time::Duration,
};

use koto::{
    runtime::{runtime_error, RuntimeError, Value, ValueList, ValueNumber},
    Koto,
};

use anyhow::{anyhow, bail, Result};
use parking_lot::RwLock;

fn main() -> Result<()> {
    let runtime = Arc::new(RwLock::new(Koto::default()));
    let (tx, rx) = channel::<String>();

    let rt = runtime.clone();
    let t1 = spawn(move || loop {
        sleep(Duration::from_millis(1000));
        if let Some(mut g) = rt.try_write() {
            println!("T1 got the lock!");
            g.compile("x = 42");
        } else {
            // println!("T1 Couldn't acquire runtime for write.")
        }
    });

    let t2 = spawn(move || loop {
        sleep(Duration::from_millis(4000));
        tx.send("message from t2".into());
    });

    loop {
        sleep(Duration::from_millis(1000));

        if let None = runtime.try_write() {
            // println!("Couldn't acquire runtime for write.")
        } else {
            println!("Main got the lock!");
        }
        match rx.recv() {
            Ok(msg) => {
                // dbg!(msg);
                continue;
            }
            Err(err) => {
                // dbg!(err);
            }
        }
    }

    Ok(())
}
