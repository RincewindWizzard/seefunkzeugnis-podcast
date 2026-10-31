//! hoerspiel-deck: an Elgato Stream Deck drives Audacity while recording the radio play.
//!
//! The program
//!  * finds the Stream Deck on its own and sets brightness and key images,
//!  * connects to Audacity (see [`audacity`] for the protocol) and creates missing tracks,
//!  * selects a speaker's track on key press, starts/stops recording, undoes,
//!  * marks the selected track with a white frame and a running recording with blinking red,
//!  * reconnects whenever the deck or Audacity went away.

#![forbid(unsafe_code)]

mod audacity;
mod icons;
mod layout;

use audacity::Audacity;
use elgato_streamdeck::{DeviceStateUpdate, StreamDeck, StreamDeckError, list_devices, new_hidapi};
use icons::KeyImages;
use layout::{Action, LAYOUT, TRACKS};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const BLINK_INTERVAL: Duration = Duration::from_millis(500);
const ALERT_DURATION: Duration = Duration::from_millis(800);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(3);
const DEFAULT_BRIGHTNESS: u8 = 70;

/// Everything the key images depend on.
#[derive(Default)]
struct State {
    audacity: Option<Audacity>,
    selected_track: Option<&'static str>,
    recording: bool,
    blink_on: bool,
    /// Layout index of a key that shows a red alert, and until when.
    alert: Option<(usize, Instant)>,
}

impl State {
    /// Connects to Audacity if not connected yet. Returns whether a connection exists.
    fn ensure_audacity(&mut self) -> bool {
        if self.audacity.is_some() {
            return true;
        }
        match Audacity::connect() {
            Ok(mut a) => {
                println!("Connected to Audacity.");
                match a.ensure_tracks(&TRACKS) {
                    Ok(0) => {}
                    Ok(n) => println!("Created {n} track(s)."),
                    Err(e) => eprintln!("Creating tracks failed: {e}"),
                }
                self.audacity = Some(a);
                true
            }
            Err(_) => false,
        }
    }

    fn disconnect(&mut self, reason: &dyn std::fmt::Display) {
        eprintln!("Lost connection to Audacity: {reason}");
        self.audacity = None;
        self.recording = false;
    }

    /// Runs an action. Returns `false` if it failed, so the key can flash red.
    fn run(&mut self, action: Action) -> bool {
        if !self.ensure_audacity() {
            eprintln!("Audacity is not reachable. Is it running with mod-script-pipe enabled?");
            return false;
        }
        let Some(a) = self.audacity.as_mut() else {
            return false;
        };

        let result = match action {
            Action::Track(name) => a.select_track(name),
            Action::Record if self.recording => a.command("Stop:").map(|_| true),
            Action::Record => a.command("Record1stChoice:").map(|_| true),
            Action::Stop => a.command("Stop:").map(|_| true),
            Action::Undo => a.command("Undo:").map(|_| true),
        };

        match result {
            Ok(true) => {
                match action {
                    Action::Track(name) => self.selected_track = Some(name),
                    Action::Record => self.recording = !self.recording,
                    Action::Stop => self.recording = false,
                    Action::Undo => {}
                }
                true
            }
            Ok(false) => {
                eprintln!("Action {action:?} failed: track not found in Audacity.");
                false
            }
            Err(e) => {
                self.disconnect(&e);
                false
            }
        }
    }

    fn image_for<'a>(&self, index: usize, images: &'a KeyImages) -> &'a image::DynamicImage {
        if matches!(self.alert, Some((i, _)) if i == index) {
            return &images.alert;
        }
        if self.audacity.is_none() {
            return &images.dimmed;
        }
        match LAYOUT[index].action {
            Action::Track(name) if self.selected_track == Some(name) => &images.active,
            Action::Record if self.recording && self.blink_on => &images.alert,
            _ => &images.normal,
        }
    }
}

fn draw(
    deck: &StreamDeck,
    key_map: &[Option<usize>],
    images: &[KeyImages],
    state: &State,
) -> Result<(), StreamDeckError> {
    for (key, entry) in key_map.iter().enumerate() {
        let key = key as u8;
        match entry {
            Some(index) => {
                deck.set_button_image(key, state.image_for(*index, &images[*index]).clone())?
            }
            None => deck.clear_button_image(key)?,
        }
    }
    deck.flush()
}

fn find_deck() -> Option<StreamDeck> {
    let hid = new_hidapi()
        .map_err(|e| eprintln!("HID access failed: {e}"))
        .ok()?;
    let (kind, serial) = list_devices(&hid).into_iter().next()?;
    match StreamDeck::connect(&hid, kind, &serial) {
        Ok(deck) => {
            println!(
                "Found Stream Deck {kind:?} (serial {serial}) with {} keys.",
                kind.key_count()
            );
            Some(deck)
        }
        Err(e) => {
            eprintln!(
                "Found a Stream Deck but could not open it: {e}\n\
                 Is the udev rule installed? Is another program (e.g. StreamController) using the deck?"
            );
            None
        }
    }
}

/// Talks to one connected deck until it is unplugged or the program should quit.
fn run_deck(
    deck: StreamDeck,
    brightness: u8,
    images: &[KeyImages],
    state: &mut State,
    quit: &AtomicBool,
) {
    // The library's `get_reader` takes `&Arc<StreamDeck>`, so an `Arc` is required even though
    // everything runs on one thread.
    #[allow(clippy::arc_with_non_send_sync)]
    let deck = Arc::new(deck);
    let kind = deck.kind();
    let key_map = layout::map_keys(kind.column_count() as usize, kind.key_count() as usize);
    if key_map.iter().flatten().count() < LAYOUT.len() {
        eprintln!(
            "Note: this deck has fewer keys than the layout; some functions are not available."
        );
    }

    // Errors here are not fatal: the first draw below reports a broken connection anyway.
    let _ = deck.reset();
    let _ = deck.set_brightness(brightness);

    if !state.ensure_audacity() {
        println!("Audacity is not reachable yet; keys are dimmed until it is.");
    }
    if let Err(e) = draw(&deck, &key_map, images, state) {
        eprintln!("Stream Deck error: {e}");
        return;
    }

    let reader = deck.get_reader();
    let mut last_blink = Instant::now();
    let mut last_reconnect = Instant::now();

    while !quit.load(Ordering::SeqCst) {
        let updates = match reader.read(Some(POLL_INTERVAL)) {
            Ok(updates) => updates,
            Err(e) => {
                eprintln!("Stream Deck disconnected ({e}). Searching again …");
                return;
            }
        };

        let mut redraw = false;
        for update in updates {
            let DeviceStateUpdate::ButtonDown(key) = update else {
                continue;
            };
            let Some(Some(index)) = key_map.get(key as usize).copied() else {
                continue;
            };
            if !state.run(LAYOUT[index].action) {
                state.alert = Some((index, Instant::now() + ALERT_DURATION));
            }
            redraw = true;
        }

        if matches!(state.alert, Some((_, until)) if Instant::now() >= until) {
            state.alert = None;
            redraw = true;
        }
        if state.recording && last_blink.elapsed() >= BLINK_INTERVAL {
            state.blink_on = !state.blink_on;
            last_blink = Instant::now();
            redraw = true;
        }
        // Light the keys up again as soon as Audacity has been started.
        if state.audacity.is_none() && last_reconnect.elapsed() >= RECONNECT_INTERVAL {
            last_reconnect = Instant::now();
            redraw |= state.ensure_audacity();
        }

        if redraw && let Err(e) = draw(&deck, &key_map, images, state) {
            eprintln!("Stream Deck error: {e}");
            return;
        }
    }

    // Leave the deck dark instead of showing stale images.
    let _ = deck.clear_all_button_images();
    let _ = deck.flush();
    let _ = deck.set_brightness(0);
}

fn parse_brightness(args: &[String]) -> u8 {
    args.iter()
        .position(|a| a == "--brightness")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<u8>().ok())
        .unwrap_or(DEFAULT_BRIGHTNESS)
        .min(100)
}

fn setup_only() -> std::process::ExitCode {
    let result = Audacity::connect().and_then(|mut a| a.ensure_tracks(&TRACKS));
    match result {
        Ok(n) => {
            println!("Done. Created {n} track(s).");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Audacity is not reachable: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "hoerspiel-deck – Stream Deck controls Audacity\n\n\
             Usage:\n  \
             hoerspiel-deck [--brightness 0-100]   run (Ctrl+C quits)\n  \
             hoerspiel-deck setup                  only create the tracks in Audacity"
        );
        return std::process::ExitCode::SUCCESS;
    }
    if args.get(1).map(String::as_str) == Some("setup") {
        return setup_only();
    }

    let quit = Arc::new(AtomicBool::new(false));
    for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
        signal_hook::flag::register(signal, Arc::clone(&quit)).expect("installing signal handler");
    }

    let brightness = parse_brightness(&args);
    let images = icons::load_all();
    let mut state = State::default();
    let mut waiting_reported = false;

    println!("hoerspiel-deck running. Quit with Ctrl+C.");
    while !quit.load(Ordering::SeqCst) {
        match find_deck() {
            Some(deck) => {
                waiting_reported = false;
                run_deck(deck, brightness, &images, &mut state, &quit);
            }
            None => {
                if !waiting_reported {
                    println!("Waiting for a Stream Deck …");
                    waiting_reported = true;
                }
                sleep(Duration::from_secs(2));
            }
        }
    }
    println!("Bye.");
    std::process::ExitCode::SUCCESS
}
