use std::{
	io,
	thread,
	time::{Duration, Instant},
	sync::mpsc,
};
use crossterm::{
	event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
	execute,
	terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{
	backend::{Backend, CrosstermBackend},
	style::{Color, Style},
	widgets::{List, ListItem},
	Frame, Terminal,
};
use crate::events::Events;
use crate::keys::Command;
use std::collections::HashMap;

mod cli;
mod events;
mod command;
mod keys;

const TICK_RATE: u64 = 250; // tui repaint interval
const DEFAULT_INTERVAL: u64 = 5; // watch interval

fn main() -> Result<(), io::Error> {
	// parse args and options
	let args = cli::parse_args();
	let command = args.value_of("command").unwrap();
	let interval: u64 = *args.get_one("interval").unwrap_or(&DEFAULT_INTERVAL);
	let watch_rate = Duration::from_secs(interval);
	let keybindings = keys::parse_bindings(args.value_of("keybindings").unwrap_or(""));
	// println!("command: {}\n", command);
	// println!("keybindings: {:?}\n", keybindings);

	// setup terminal
	enable_raw_mode()?;
	let mut stdout = io::stdout();
	execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
	let backend = CrosstermBackend::new(stdout);
	let mut terminal = Terminal::new(backend)?;

	// run tui program
	let tick_rate = Duration::from_millis(TICK_RATE);
	run(&mut terminal, &keybindings, args.clone(), command.clone(), tick_rate, watch_rate)?;

	// restore terminal
	disable_raw_mode()?;
	execute!(
		terminal.backend_mut(),
		LeaveAlternateScreen,
		DisableMouseCapture
	)?;
	terminal.show_cursor()?;

	Ok(())
}

fn run<B: Backend>(
	terminal: &mut Terminal<B>,
	keybindings: &HashMap<KeyCode, Command>,
	args: clap::ArgMatches,
	command: &str,
	tick_rate: Duration,
	watch_rate: Duration,
) -> io::Result<()> {
	let mut last_tick = Instant::now();
	let (tx, rx) = mpsc::channel();
	thread::spawn(move || {
		loop {
			let command = args.value_of("command").unwrap();
			tx.send(command::output_lines(command)).unwrap();
			if watch_rate == Duration::ZERO {
				break;
			}
			thread::sleep(watch_rate);
		}
	});
	let mut events = Events::new(rx.recv().unwrap());

	loop {
		match rx.try_recv() {
			Ok(items) => events.set_items(items),
			_ => {},
		};
		terminal.draw(|f| ui(f, &mut events))?;

		let timeout = tick_rate
			.checked_sub(last_tick.elapsed())
			.unwrap_or_else(|| Duration::ZERO);
		if event::poll(timeout)? { // wait for event (keyboard input) for max time of timeout
			if let Event::Key(key) = event::read()? {
				if !keys::handle_key(key.code, keybindings, &mut events) {
					return Ok(());
				}
			}
		}
		if last_tick.elapsed() >= tick_rate {
			last_tick = Instant::now();
		}
	}
}

// TODO: simplify
fn ui<B: Backend>(f: &mut Frame<B>, events: &mut Events) {
	let items: Vec<ListItem> = events
		.items.iter()
		.map(|i| ListItem::new(i.as_ref()))
		.collect();
	let list = List::new(items)
		.style(Style::default().fg(Color::White))
		.highlight_style(Style::default().fg(Color::Black).bg(Color::White));
	f.render_stateful_widget(list, f.size(), &mut events.state);
}
