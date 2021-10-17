use std::{error::Error, path::PathBuf};

use console::Term;
use crossterm::style::*;

const MSG: &str = "choose and then press \"enter\":";

pub struct Tui {
    stdout: Term,
}
impl Tui {
    pub fn new() -> Self {
        Self {
            stdout: Term::stdout(),
        }
    }

    fn clear_lines(&self, lines: usize) -> Result<(), Box<dyn Error>> {
        self.stdout.move_cursor_up(lines)?;
        self.stdout.clear_line()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<(), Box<dyn Error>> {
        self.stdout.clear_screen()?;
        Ok(())
    }
    fn write_line(&self, line: StyledContent<&str>) -> Result<(), Box<dyn Error>> {
        self.stdout.write_line(&format!("{}", line))?;
        Ok(())
    }

    pub fn intro(&self) -> Result<(), Box<dyn Error>> {
        self.write_line("Here are your event processor scripts,".blue())?;
        Ok(())
    }
    pub fn empty_scripts_folder(&self) -> Result<(), Box<dyn Error>> {
        self.clear_lines(1)?;
        self.write_line(
            "💡 There are no event processor scripts found in \"~/.mep\". Maybe put a couple?"
                .blue(),
        )?;
        Ok(())
    }
    pub fn show_error(&self, err: &str) -> Result<(), Box<dyn Error>> {
        self.clear_lines(1)?;
        self.write_line(format!("💡 There is an error in: {}", err)[..].magenta())?;
        self.write_line(
            "Either choose another one by entering a valid digit or fix your script.".blue(),
        )?;
        Ok(())
    }

    pub fn removed_scripts_folder(&self) -> Result<(), Box<dyn Error>> {
        self.clear_lines(1)?;
        self.write_line(
            "💡 \"~/.mep\" folder is removed. Re-run \"mep\" to auto create it and fill it with example scripts."
                .red(),
        )?;
        Ok(())
    }
    pub fn reset_scripts_folder(&self) -> Result<(), Box<dyn Error>> {
        self.clear_lines(1)?;
        self.write_line("💡 \"~/.mep\" folder is reset with example scripts.".red())?;
        Ok(())
    }

    pub fn scripts_folder_not_found(&self) -> Result<(), Box<dyn Error>> {
        self.write_line(
            "💡 Scripts folder \"~/.mep\" was not found. \"mep\" has created it and filled it with some example scripts for you.".yellow(),
        )?;
        Ok(())
    }
    pub fn elements_to_choose(&self, index: &str, name: &str) -> Result<(), Box<dyn Error>> {
        self.write_line(index.yellow())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(3)?;
        self.write_line(name.red())?;
        Ok(())
    }

    pub fn wait_for_choice(&self) -> Result<(), Box<dyn Error>> {
        // self.stdout.write_line("\n")?;
        let msg = "choose and then press \"enter\":";
        self.write_line(msg.green())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(msg.len() + 1)?;
        Ok(())
    }

    pub fn ignore_choice(&self) -> Result<(), Box<dyn Error>> {
        self.stdout.move_cursor_up(1)?;
        self.stdout.clear_line()?;
        self.write_line(MSG.green())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(MSG.len() + 1)?;

        Ok(())
    }
    pub fn highlight_and_render(
        &self,
        index: &str,
        available_scripts: &[String],
    ) -> Result<(), Box<dyn Error>> {
        let index_as_number: usize = index.parse().unwrap();
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
                format!("{:?}", PathBuf::from(element).file_name().unwrap())[..].red(),
            )?;
        }

        self.write_line(MSG.green())?;
        self.stdout.move_cursor_up(1)?;
        self.stdout.move_cursor_right(MSG.len() + 1)?;

        Ok(())
    }
}
