use crate::config::Config;
use crate::exec::output_lines;
use crate::keybindings::handle_key;
use crate::state::State;
use crate::terminal_manager::{Terminal, TerminalManager};
use anyhow::Result;
use crossterm::event::{self, Event::Key, KeyCode};
use mpsc::{Receiver, Sender};
use std::{
	sync::mpsc,
	thread,
	time::{Duration, Instant},
};

pub enum RequestedAction {
	Continue,
	Reload,
	Block(Receiver<Result<()>>),
	Exit,
}

enum Event {
	KeyPressed(KeyCode),
	CommandOutput(Result<Vec<String>>),
}

struct Blocking {
	waiting_for: Vec<Receiver<Result<()>>>,
}

impl Blocking {
	pub fn new() -> Blocking {
		Blocking {
			waiting_for: vec![],
		}
	}

	pub fn add(&mut self, rx: Receiver<Result<()>>) {
		self.waiting_for.push(rx);
	}

	pub fn ready(&mut self) -> Result<bool> {
		let mut delete: Vec<usize> = vec![];
		// TODO: combine into one for loop
		for (i, rx) in self.waiting_for.iter().enumerate() {
			if let Ok(err_msg) = rx.try_recv() {
				err_msg?;
				delete.push(i);
			}
		}
		for i in delete {
			self.waiting_for.swap_remove(i);
		}
		Ok(self.waiting_for.is_empty())

		// self.waiting_for.retain(|rx| {
		// 	match rx.try_recv() {
		// 		Ok(_) => true,
		// 		Err(_) => false,
		// 	}
		// });
	}
}

pub fn start(config: Config) -> Result<()> {
	let mut terminal_manager = TerminalManager::new()?;
	let err = run(config, &mut terminal_manager.terminal);
	terminal_manager.restore()?;
	err
}

fn run(config: Config, terminal: &mut Terminal) -> Result<()> {
	let (event_tx, event_rx) = mpsc::channel();
	let (wake_tx, wake_rx) = mpsc::channel();
	let mut state = State::new(&config.styles);
	let mut blocking = Blocking::new();

	poll_execute_command(
		config.watch_rate.clone(),
		config.command.clone(),
		event_tx.clone(),
		wake_rx,
	);
	poll_key_events(event_tx.clone());

	loop {
		terminal.draw(|frame| state.draw(frame))?;

		match event_rx.recv() {
			Ok(Event::KeyPressed(key)) => {
				if blocking.ready()? {
					for requested_state in handle_key(key, &config.keybindings, &mut state)? {
						match requested_state {
							RequestedAction::Exit => return Ok(()),
							// reload input by waking up thread
							RequestedAction::Reload => wake_tx.send(()).unwrap(),
							RequestedAction::Block(block_rx) => blocking.add(block_rx),
							RequestedAction::Continue => {},
						}
					}
				}
			}
			// TODO: possible inefficiency: blocks (due to blocking subshell command), but continues executing and sending CommandOutputs => old set_lines will be called even though new ones are available
			// TODO: solution: enter blocking state where all key events are ignored but command output is still handled (implement with another channel?)
			Ok(Event::CommandOutput(lines)) => state.set_lines(lines?),
			_ => {}
		};
	}
}

fn poll_execute_command(
	watch_rate: Duration,
	command: String,
	event_tx: Sender<Event>,
	wake_rx: Receiver<()>,
) {
	thread::spawn(move || {
		loop {
			// execute command and time execution
			let before = Instant::now();
			let lines = output_lines(&command);
			let exec_time = Instant::now().duration_since(before);
			let sleep = watch_rate.saturating_sub(exec_time);

			// ignore error that occurs when main thread (and channels) close
			event_tx.send(Event::CommandOutput(lines)).ok();

			// sleep until notified
			if watch_rate == Duration::ZERO {
				wake_rx.recv().ok();
			} else {
				// wake up at latest after watch_rate time
				wake_rx.recv_timeout(sleep).ok();
			}
		}
	});
}

fn poll_key_events(tx: Sender<Event>) {
	thread::spawn(move || {
		loop {
			// TODO: remove unwraps
			if let Key(key) = event::read().unwrap() {
				tx.send(Event::KeyPressed(key.code)).unwrap();
			}
		}
	});
}
