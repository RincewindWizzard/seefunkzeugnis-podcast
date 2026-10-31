//! Client for Audacity's scripting interface (`mod-script-pipe`).
//!
//! # How the protocol works
//!
//! When the module `mod-script-pipe` is enabled (Edit → Preferences → Modules, then restart
//! Audacity), Audacity starts a small "pipe server" thread. On Linux and macOS it uses two
//! named pipes (FIFOs) in `/tmp`, one per direction, named after the user id:
//!
//! | File                                  | Direction          | We…            |
//! |---------------------------------------|--------------------|----------------|
//! | `/tmp/audacity_script_pipe.to.<uid>`  | client → Audacity  | write commands |
//! | `/tmp/audacity_script_pipe.from.<uid>`| Audacity → client  | read responses |
//!
//! (On Windows the same protocol runs over `\\.\pipe\ToSrvPipe` and `\\.\pipe\FromSrvPipe`.)
//!
//! ## Opening order matters
//!
//! The server thread first opens the *to* pipe for reading. Opening a FIFO blocks until the
//! other side opens it too, so Audacity waits there until a client connects. Only then does it
//! open the *from* pipe for writing. A client therefore has to open **to first, then from**.
//! The other order deadlocks: both sides would wait on different pipes.
//!
//! The FIFO files stay in `/tmp` after Audacity quits. A plain blocking `open` on such a stale
//! pipe would hang forever, because nobody is reading. We therefore open the *to* pipe with
//! `O_NONBLOCK`. For a FIFO without a reader, the kernel then refuses immediately with `ENXIO`
//! ("No such device or address"), which tells us Audacity is not running. After a successful
//! open we switch the descriptor back to blocking mode, so writes behave normally.
//!
//! ## Request / response
//!
//! The conversation is strictly one command at a time:
//!
//! 1. The client writes **one line**: the command's scripting id, a colon and optional
//!    `Key=Value` parameters, then a newline. Values containing spaces go in double quotes:
//!
//!    ```text
//!    SelectTracks: Track=3 TrackCount=1 Mode=Set
//!    SetTrackStatus: Name="Funk Klaas"
//!    ```
//!
//!    The ids are the same ones Audacity's macro editor shows (Tools → Macros).
//!
//! 2. Audacity runs the command and answers with
//!    * zero or more lines of output (for example a JSON document for `GetInfo`),
//!    * a status line `BatchCommand finished: OK` (or `… Failed!` if the command failed),
//!    * an **empty line**, which marks the end of the response.
//!
//!    The client must read up to that empty line before sending the next command; otherwise
//!    responses get out of step with their commands.
//!
//! If Audacity quits, the read side reports end of file. The client then drops the connection
//! and connects again later.
//!
//! ## Commands used here
//!
//! | Command                                   | Purpose                                          |
//! |-------------------------------------------|--------------------------------------------------|
//! | `GetInfo: Type=Tracks Format=JSON`        | list all tracks (name, kind, selected, …)        |
//! | `NewMonoTrack:`                           | append an empty mono track                       |
//! | `SelectTracks: Track=n TrackCount=1 Mode=Set` | select exactly track `n` (0-based, top to bottom) |
//! | `SetTrackStatus: Name="…"` / `Focused=1`  | rename / focus the selected track                |
//! | `SelectNone:`                             | clear the selection                              |
//! | `Record1stChoice:`                        | record, same as pressing `R`                     |
//! | `Stop:`                                   | stop playback or recording                       |
//! | `Undo:`                                   | undo the last action                             |
//!
//! Track numbers depend on the current order in the project, so we always look tracks up by
//! **name** via `GetInfo` right before selecting them. The user can reorder tracks freely.

use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

/// Status line prefix that ends every response (before the terminating empty line).
const STATUS_PREFIX: &str = "BatchCommand finished:";

/// An open connection to a running Audacity.
pub struct Audacity {
    to_audacity: File,
    from_audacity: BufReader<File>,
}

/// Uid of the current user, needed for the pipe names.
///
/// Read from the owner of `/proc/self`, which avoids an `unsafe` call to `getuid()`.
fn current_uid() -> io::Result<u32> {
    Ok(fs::metadata("/proc/self")?.uid())
}

impl Audacity {
    /// Connects to Audacity. Fails quickly (instead of hanging) if Audacity is not running.
    pub fn connect() -> io::Result<Audacity> {
        let uid = current_uid()?;
        let to_path = format!("/tmp/audacity_script_pipe.to.{uid}");
        let from_path = format!("/tmp/audacity_script_pipe.from.{uid}");

        // Step 1: the "to" pipe, non-blocking so a stale FIFO gives ENXIO instead of a hang.
        let to_audacity = OpenOptions::new()
            .write(true)
            .custom_flags(OFlags::NONBLOCK.bits() as i32)
            .open(&to_path)?;
        // Back to blocking mode: a command must never be cut short with EAGAIN.
        let flags = fcntl_getfl(&to_audacity)?;
        fcntl_setfl(&to_audacity, flags - OFlags::NONBLOCK)?;

        // Step 2: the "from" pipe. Audacity opens its end right after we connected above.
        let from_audacity = BufReader::new(File::open(&from_path)?);

        Ok(Audacity {
            to_audacity,
            from_audacity,
        })
    }

    /// Sends one command and returns all response lines (including the status line).
    pub fn command(&mut self, command: &str) -> io::Result<Vec<String>> {
        writeln!(self.to_audacity, "{command}")?;
        self.to_audacity.flush()?;

        let mut lines = Vec::new();
        loop {
            let mut line = String::new();
            if self.from_audacity.read_line(&mut line)? == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Audacity closed the connection",
                ));
            }
            // An empty line after at least one line of output ends the response.
            if line == "\n" && !lines.is_empty() {
                break;
            }
            lines.push(line.trim_end_matches('\n').to_string());
        }

        if let Some(status) = lines.last()
            && status.starts_with(STATUS_PREFIX)
            && !status.ends_with("OK")
        {
            eprintln!("Audacity reported: {status} (command: {command})");
        }
        Ok(lines)
    }

    /// Names of all tracks, top to bottom. The index in the result is Audacity's track number.
    pub fn track_names(&mut self) -> io::Result<Vec<String>> {
        let response = self.command("GetInfo: Type=Tracks Format=JSON")?;
        let json: Vec<&str> = response
            .iter()
            .map(String::as_str)
            .filter(|line| !line.starts_with(STATUS_PREFIX))
            .collect();
        let value: serde_json::Value = serde_json::from_str(&json.join("\n")).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unreadable track list: {e}"),
            )
        })?;

        let tracks = value.as_array().map(Vec::as_slice).unwrap_or_default();
        Ok(tracks
            .iter()
            .map(|track| {
                track
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect())
    }

    /// Creates every track from `names` that does not exist yet. Returns how many were created.
    pub fn ensure_tracks(&mut self, names: &[&str]) -> io::Result<usize> {
        let existing = self.track_names()?;
        let mut created = 0;
        for name in names {
            if existing.iter().any(|e| e.trim().eq_ignore_ascii_case(name)) {
                continue;
            }
            self.command("NewMonoTrack:")?;
            // The new track is the last one.
            let last = self.track_names()?.len() - 1;
            self.command(&format!("SelectTracks: Track={last} TrackCount=1 Mode=Set"))?;
            self.command(&format!("SetTrackStatus: Name=\"{name}\""))?;
            created += 1;
        }
        if created > 0 {
            self.command("SelectNone:")?;
        }
        Ok(created)
    }

    /// Selects exactly the track called `name`. Returns `false` if there is no such track.
    pub fn select_track(&mut self, name: &str) -> io::Result<bool> {
        let tracks = self.track_names()?;
        let Some(index) = tracks
            .iter()
            .position(|t| t.trim().eq_ignore_ascii_case(name))
        else {
            return Ok(false);
        };
        self.command(&format!(
            "SelectTracks: Track={index} TrackCount=1 Mode=Set"
        ))?;
        self.command("SetTrackStatus: Focused=1")?;
        Ok(true)
    }
}
