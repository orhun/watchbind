use crate::exec;
use crate::stateful_list::StatefulList;
use crossterm::event::KeyCode::{self, *};
use std::{
	collections::HashMap,
	io::{Error, ErrorKind},
	str::FromStr,
	sync::mpsc,
};

pub type Operations = Vec<Operation>;
pub type KeybindingsRaw = HashMap<String, Vec<String>>;
pub type Keybindings = HashMap<KeyCode, Operations>;

// TODO: add support for goto nth line
#[derive(Clone)]
pub enum MoveCursor {
	Down(usize),
	Up(usize),
	First,
	Last,
}

#[derive(Clone)]
pub enum SelectOperation {
	Select,
	Unselect,
	Toggle,
	SelectAll,
	UnselectAll,
}

#[derive(Clone)]
pub struct Command {
	pub command: String,
	// execute as background process or wait for termination
	pub background: bool,
}

// TODO: extract select and toggle into one type
#[derive(Clone)]
pub enum Operation {
	Exit,
	Reload,
	MoveCursor(MoveCursor),
	SelectLine(SelectOperation),
	Execute(Command),
}

// TODO: return (&str, &str), deal with lifetime
pub fn parse_str(s: &str) -> Result<(String, Vec<String>), Error> {
	// TODO: replace with nom
	let (key, operations) = s.split_once(':').ok_or_else(|| {
		Error::new(
			ErrorKind::Other,
			format!("invalid format: expected \"KEY:OP[+OP]*\", found \"{}\"", s),
		)
	})?;
	Ok((
		key.to_string(),
		// split on "+" and trim leading and trailing whitespace
		operations
			.split('+')
			.map(|op| op.trim().to_string())
			.collect(),
	))
}

pub fn parse_raw(raw: KeybindingsRaw) -> Result<Keybindings, Error> {
	raw
		.into_iter()
		.map(|(key, ops)| Ok((keycode_from_str(&key)?, operations_from_str(ops)?)))
		.collect()
}

fn operations_from_str(ops: Vec<String>) -> Result<Vec<Operation>, Error> {
	ops.iter().map(|op| Ok(Operation::from_str(op)?)).collect()
}

// new and old have same key => keep new value
pub fn merge_raw(new: KeybindingsRaw, old: KeybindingsRaw) -> KeybindingsRaw {
	let mut merged = old.clone();
	merged.extend(new);
	merged
}

fn exec_operation(
	operation: &Operation,
	state: &mut StatefulList,
	thread_channel: &mpsc::Sender<()>,
) -> Result<bool, Error> {
	match operation {
		Operation::MoveCursor(MoveCursor::Down(steps)) => state.down(*steps),
		Operation::MoveCursor(MoveCursor::Up(steps)) => state.up(*steps),
		Operation::MoveCursor(MoveCursor::First) => state.first(),
		Operation::MoveCursor(MoveCursor::Last) => state.last(),
		Operation::SelectLine(SelectOperation::Select) => state.select(),
		Operation::SelectLine(SelectOperation::Unselect) => state.unselect(),
		Operation::SelectLine(SelectOperation::Toggle) => state.select_toggle(),
		Operation::SelectLine(SelectOperation::SelectAll) => state.select_all(),
		Operation::SelectLine(SelectOperation::UnselectAll) => state.unselect_all(),
		Operation::Execute(command) => exec::run_line(command, state.get_selected_line())?,
		// reload input by waking up thread
		Operation::Reload => thread_channel.send(()).unwrap(),
		Operation::Exit => return Ok(false),
	};
	Ok(true)
}

pub fn handle_key(
	key: KeyCode,
	keybindings: &Keybindings,
	state: &mut StatefulList,
	thread_channel: &mpsc::Sender<()>,
) -> Result<bool, Error> {
	if let Some(operations) = keybindings.get(&key) {
		for op in operations {
			if !exec_operation(op, state, thread_channel)? {
				// exit was called => program should be stopped
				return Ok(false);
			}
		}
	}
	Ok(true)
}

impl FromStr for Operation {
	type Err = Error;
	fn from_str(src: &str) -> Result<Operation, Self::Err> {
		Ok(
			// TODO: make more efficient by removing collect
			match src.split_whitespace().collect::<Vec<&str>>()[..] {
				["exit"] => Operation::Exit,
				["reload"] => Operation::Reload,
				["down"] => Operation::MoveCursor(MoveCursor::Down(1)),
				["up"] => Operation::MoveCursor(MoveCursor::Up(1)),
				// TODO: add custom error type with error handling to make less ugly
				["down", steps] => match steps.parse() {
					Ok(steps) => return Ok(Operation::MoveCursor(MoveCursor::Down(steps))),
					Err(_) => {
						return Err(Error::new(
							ErrorKind::Other,
							format!(
								"Invalid integer step size \"{}\" provided in keybinding: \"{}\"",
								steps, src
							),
						))
					}
				},
				["up", steps] => match steps.parse() {
					Ok(steps) => return Ok(Operation::MoveCursor(MoveCursor::Up(steps))),
					Err(_) => {
						return Err(Error::new(
							ErrorKind::Other,
							format!(
								"Invalid integer step size \"{}\" provided in keybinding: \"{}\"",
								steps, src
							),
						))
					}
				},
				["first"] => Operation::MoveCursor(MoveCursor::First),
				["last"] => Operation::MoveCursor(MoveCursor::Last),
				["select"] => Operation::SelectLine(SelectOperation::Select),
				["unselect"] => Operation::SelectLine(SelectOperation::Unselect),
				["select-toggle"] => Operation::SelectLine(SelectOperation::Toggle),
				["select-all"] => Operation::SelectLine(SelectOperation::SelectAll),
				["unselect-all"] => Operation::SelectLine(SelectOperation::UnselectAll),
				_ => Operation::Execute(Command {
					command: src.to_string(),
					background: src.contains("&"),
				}),
			},
		)
	}
}

// TODO: add modifiers
fn keycode_from_str(s: &str) -> Result<KeyCode, Error> {
	Ok(match s {
		"esc" => Esc,
		"enter" => Enter,
		"left" => Left,
		"right" => Right,
		"up" => Up,
		"down" => Down,
		"home" => Home,
		"end" => End,
		"pageup" => PageUp,
		"pagedown" => PageDown,
		"backtab" => BackTab,
		"backspace" => Backspace,
		"del" => Delete,
		"delete" => Delete,
		"insert" => Insert,
		"ins" => Insert,
		"f1" => F(1),
		"f2" => F(2),
		"f3" => F(3),
		"f4" => F(4),
		"f5" => F(5),
		"f6" => F(6),
		"f7" => F(7),
		"f8" => F(8),
		"f9" => F(9),
		"f10" => F(10),
		"f11" => F(11),
		"f12" => F(12),
		"space" => Char(' '),
		"tab" => Tab,
		c if c.len() == 1 => Char(c.chars().next().unwrap()),
		invalid => {
			return Err(Error::new(
				ErrorKind::Other,
				format!("Invalid key provided in keybinding: {}", invalid),
			))
		}
	})
}

// TODO: idea: parse from file instead of hardcoded
pub fn default_raw() -> KeybindingsRaw {
	[
		("q", vec!["exit"]),
		("r", vec!["reload"]),
		("space", vec!["select-toggle", "down"]),
		("v", vec!["select-toggle"]),
		("esc", vec!["unselect-all"]),
		("down", vec!["down"]),
		("up", vec!["up"]),
		("j", vec!["down"]),
		("k", vec!["up"]),
		("g", vec!["first"]),
		("G", vec!["last"]),
	]
	.iter()
	.map(|(key, commands)| {
		(
			key.to_string(),
			commands.iter().map(|cmd| cmd.to_string()).collect(),
		)
	})
	.collect()
}
