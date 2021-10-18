# mep

## Introduction

**mep** is a scriptable midi event processor.

It uses [koto](https://github.com/koto-lang/koto) scripts to process incoming midi events.

### I/O

Every instance of **mep** introduces a new virtual **midi-in** and a virtual **midi-out** port for the incoming messages to be processed and the processed messages to be sent out.

The default naming for these virtual ports are `mep_in` and `mep_out`.

If multiple instances of **mep** are running, every additional instance's virtual ports are suffixed with an increasing number such as `mep_in #2`,`mep_out #2`,`mep_in #3`,`mep_out #3` and so on..

One can name an instance's port by using the `--port <port name>` option when running **mep**.

For example, `mep --port wakkanai` would create `wakkanai_in` and `wakkanai_out` virtual ports.

### Scripts

**mep** would check for `.mep` folder in your **home directory** and shows you an enumerated list of all files (_scripts_) in the folder with the extension of `.koto`.

If no `.mep` folder is found on startup, it will create one and fill it with bunch of example scripts.

If `.mep` folder exists but empty, **mep** will notify you about this, ask you to add some scripts and exit.

As soon as you select a script from the enumerated list by entering the index number and pressing enter, the virtual ports will be created and the event processing will begin.

### Editing

When an instance of **mep** is running. `.mep` folder is being watched for changes. Editing your scripts will be reflected immediately.

Look for info in the [koto main repository](https://github.com/koto-lang/koto) to see if there is syntax highlighting available for your editor.

## Some answers to possible questions

- _What are koto scripts?_

  Koto is a simple, expressive, embeddable programming language, made with Rust.

  Please visit the [main repository](https://github.com/koto-lang/koto) for more information.

- _How to process midi with koto?_

  I've written a [midi library](https://github.com/alisomay/koto_midi) (_toolkit_) for koto. Koto scripts which are run by **mep** have access to this library.

  In the example scripts you will notice `import midi` statement in the begining.

- _Are there any examples?_

  Please check the `example_scripts` in the repository to see some working examples.

- _How may I see what is available in `koto_midi` library to use?_

  Currently if you visit the [repository](https://github.com/alisomay/koto_midi) and follow the instructions to run tests, a complete API will be printed to `stdout`. You can take this as a reference. For sure in the future a more ergonomic way will be introduced.

- _How may I debug my scripts?_

  Errors in your scripts would navigate you to a new screen and show you the koto error.
  **mep** will let you know which script caused the error and wait for changes.

  After you fixed your erroring script in `~/.mep` folder, it will move to the screen where you can choose your scripts again.

  Please check [todo](###Todo) section for some exceptions to this behavior.

## Building

Building **mep** is as simple as running `cargo build` or `cargo build --release` if you have **rust** in your system installed.

**mep** uses the nightly channel or rust.

## Running

To run **mep** after building you may run `cargo run` if you have **rust** in your system installed.
You may add command line options by running `cargo run -- <your-command-line-options>`.
To see the list of available command line options you may run `cargo run -- --help`.

Alternatively if you run `cargo build` and then navigate to `<repository-root>/target/debug` you may find the `mep` binary and run it or if you run `cargo build --release` then the binary will be in `<repository-root>/target/release`.

## Extra

**mep** also exposes `random` library from koto.
You may visit [koto main repository](https://github.com/koto-lang/koto) to understand how to use it.

You may bring it in scope by writing `import random` in koto scripts.

## Last words

Currently **mep** is in early development stage and not stable.
A release will be made when required stability is achieved.

### Todo

- Do not panic in `runtime_error!` macros. Fall back to watcher and notify user to fix the script.
- Improve code quality.
- Add tests where possible.
