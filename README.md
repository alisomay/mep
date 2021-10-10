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

## Some answers to possible questions

- _What are koto scripts?_

  Koto is a simple, expressive, embeddable programming language, made with Rust.

  Please visit the [main repository](https://github.com/koto-lang/koto) for more information.

- _How do we process midi with koto?_

  I've written a [midi library](https://github.com/alisomay/koto_midi) (_toolkit_) for koto. Koto scripts which are run by **mep** have access to this library.

  In the example scripts you will notice `import midi` statement in the begining.

- _Are there any examples?_

  Please check the `example_scripts` in the repository to see some working examples.

- _How may I see what is available in `koto_midi` library to use?_

  Currently if you visit the [repository](https://github.com/alisomay/koto_midi) and follow the instructions to run tests, a complete API will be printed to `stdout`. You can take this as a reference. For sure in the future a more ergonomic way will be introduced.

- _How may do I debug my scripts?_

  Currently errors in your scripts would cause **mep** to panic.
  This behavior will be replaced soon with ignoring errors.
  **mep** will let you know which script caused the error.
  Then it will suggest you to run it with the upcoming `--debug <script_name>` option to supply you an ergonomic environment for debugging koto scripts.

## Building

Building **mep** is as simple as running `cargo build` if you have **rust** in your system installed.

## Extra

**mep** also exposes `random` library from koto.
You may visit [koto main repository](https://github.com/koto-lang/koto) to understand how to use it.

You may bring it in scope by writing `import random` in koto scripts.

## Last words

Currently **mep** is in early development stage and not stable.
A release will be made when required stability is achieved.
