use std::path::PathBuf;

use console::Term;
use crossterm::style::{Attribute, StyledContent, Stylize};

use anyhow::Result;

const VALUE_ENTRY_LINE: &str = "Please choose a script to run and start watching for changes.\nType a digit from the list and then press \"enter\":";
const VALUE_ENTRY_LINE_LENGTH: usize =
    "\nType a digit from the list and then press \"enter\":".len();
pub const BULB: &str = "\u{1f4a1}";

pub struct Tui {
    stdout: Term,
}
impl Tui {
    pub fn new() -> Self {
        Self {
            stdout: Term::stdout(),
        }
    }

    pub fn clear_lines(&self, lines: usize) -> Result<()> {
        self.stdout.move_cursor_up(lines)?;
        self.stdout.clear_line()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        self.stdout.clear_screen()?;
        Ok(())
    }
    fn write_line(&self, line: StyledContent<&str>) -> Result<()> {
        self.stdout.write_line(&format!("{}", line))?;
        Ok(())
    }

    pub fn intro(&self) -> Result<()> {
        self.write_line("Here are your event processor scripts,".blue())?;
        Ok(())
    }
    // pub fn no_home(&self) -> Result<()> {
    //     self.write_line("\"mep\" couldn't determine your home directory, to help it please run it with \"--home <absolute-path-to-your-home-directory>\"".blue())?;
    //     Ok(())
    // }
    // pub fn empty_scripts_folder(&self) -> Result<()> {
    //     self.clear_lines(1)?;
    //     self.write_line(
    //         format!(
    //             "{} There are no event processor scripts found in \"~/.mep\". Maybe put a couple?",
    //             BULB
    //         )[..]
    //             .blue(),
    //     )?;
    //     Ok(())
    // }
    pub fn show_error(&self, info: &str, err: &str) -> Result<()> {
        self.clear_lines(1)?;
        self.write_line(format!("{} There is an error in: {}", BULB, info)[..].magenta())?;
        self.write_line("Please navigate to the \"~/.mep\" folder and fix your script.".blue())?;
        self.write_line("".blue())?;
        self.write_line(err.white().attribute(Attribute::Framed))?;
        Ok(())
    }

    pub fn removed_scripts_folder(&self) -> Result<()> {
        self.clear_lines(1)?;
        self.write_line(
            format!("{} \"~/.mep\" folder is removed. Re-run \"mep\" to auto create it and fill it with example scripts.",BULB)[..]
                .red(),
        )?;
        Ok(())
    }
    pub fn reset_scripts_folder(&self) -> Result<()> {
        self.clear_lines(1)?;
        self.write_line(
            format!("{} \"~/.mep\" folder is reset with example scripts.", BULB)[..].red(),
        )?;
        Ok(())
    }

    pub fn scripts_folder_not_found(&self) -> Result<()> {
        self.write_line(
            format!("{} Scripts folder \"~/.mep\" was not found. \"mep\" has created it and filled it with some example scripts for you.", BULB)[..].yellow(),
        )?;
        Ok(())
    }

    pub fn ignore_choice(&self) -> Result<()> {
        self.stdout.move_cursor_up(1)?;
        self.stdout.clear_line()?;
        self.write_line(VALUE_ENTRY_LINE.green())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(VALUE_ENTRY_LINE_LENGTH)?;

        Ok(())
    }

    pub fn list_scripts(&self, available_scripts: &[String]) -> Result<()> {
        self.clear()?;
        self.intro()?;
        for (i, element) in available_scripts.iter().enumerate() {
            self.write_line(i.to_string().as_str().yellow())?;

            self.stdout.move_cursor_up(1)?;
            self.stdout.move_cursor_right(3)?;
            self.write_line(
                format!(
                    "{:?}",
                    // Returns empty string if fails.
                    PathBuf::from(element).file_name().unwrap_or_default()
                )[..]
                    .red(),
            )?;
        }

        self.write_line(VALUE_ENTRY_LINE.green())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(VALUE_ENTRY_LINE_LENGTH)?;

        Ok(())
    }

    pub fn highlight_and_render(&self, index: &str, available_scripts: &[String]) -> Result<()> {
        let index_as_number: usize = index.parse()?;
        // self.stdout.clear_last_lines(available_scripts.len() + 1)?;

        self.clear()?;
        self.intro()?;
        for (i, element) in available_scripts.iter().enumerate() {
            if i == index_as_number {
                self.write_line(i.to_string().as_str().green())?;
            } else {
                self.write_line(i.to_string().as_str().yellow())?;
            }

            self.stdout.move_cursor_up(1)?;
            self.stdout.move_cursor_right(3)?;
            self.write_line(
                format!(
                    "{:?}",
                    // Returns empty string if fails.
                    PathBuf::from(element).file_name().unwrap_or_default()
                )[..]
                    .red(),
            )?;
        }

        self.write_line(VALUE_ENTRY_LINE.green())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(VALUE_ENTRY_LINE_LENGTH)?;

        Ok(())
    }
}
