//! hoerspiel-deck: Stream Deck steuert Audacity (mod-script-pipe) beim Einsprechen des Hörspiels.
//!
//! Das Programm
//!  * findet das Stream Deck selbst, setzt Helligkeit und Tastenbilder,
//!  * verbindet sich mit Audacity und legt fehlende Spuren automatisch an,
//!  * wählt per Tastendruck die Spur des Sprechers aus, startet/stoppt die Aufnahme, macht Rückgängig,
//!  * zeigt die aktive Spur mit weißem Rahmen und eine laufende Aufnahme mit blinkendem Rot,
//!  * verbindet sich neu, wenn Deck oder Audacity weg waren.

use elgato_streamdeck::{list_devices, new_hidapi, DeviceStateUpdate, StreamDeck};
use image::{DynamicImage, Rgba, RgbaImage};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Konfiguration
// ---------------------------------------------------------------------------

/// Reihenfolge der Spuren, wie sie in Audacity angelegt werden.
const SPUREN: [&str; 12] = [
    "Erzähler",
    "Hinnerk",
    "Jonas",
    "Birte",
    "Ole",
    "Klaas",
    "Nordic Star",
    "Bremen Rescue",
    "Funk Hinnerk",
    "Funk Klaas",
    "Funk Nordic Star",
    "Funk Bremen Rescue",
];

#[derive(Clone, Copy, PartialEq)]
enum Aktion {
    Spur(&'static str),
    Aufnahme,
    Stopp,
    Rueckgaengig,
}

/// Belegung: 5 Spalten x 3 Zeilen (Stream Deck MK.2 / Original).
/// Bei anderen Deck-Größen wird das Raster oben links platziert.
const SPALTEN: usize = 5;
const BELEGUNG: [(Aktion, &[u8]); 15] = [
    // Zeile 1
    (Aktion::Spur("Erzähler"), include_bytes!("../assets/01_erzaehler.png")),
    (Aktion::Spur("Hinnerk"), include_bytes!("../assets/02_hinnerk.png")),
    (Aktion::Spur("Jonas"), include_bytes!("../assets/03_jonas.png")),
    (Aktion::Spur("Birte"), include_bytes!("../assets/04_birte.png")),
    (Aktion::Spur("Ole"), include_bytes!("../assets/05_ole.png")),
    // Zeile 2
    (Aktion::Spur("Klaas"), include_bytes!("../assets/06_klaas.png")),
    (Aktion::Spur("Nordic Star"), include_bytes!("../assets/07_nordic_star.png")),
    (Aktion::Spur("Bremen Rescue"), include_bytes!("../assets/08_bremen_rescue.png")),
    (Aktion::Aufnahme, include_bytes!("../assets/13_aufnahme.png")),
    (Aktion::Stopp, include_bytes!("../assets/14_stopp.png")),
    // Zeile 3
    (Aktion::Spur("Funk Hinnerk"), include_bytes!("../assets/09_funk_hinnerk.png")),
    (Aktion::Spur("Funk Klaas"), include_bytes!("../assets/10_funk_klaas.png")),
    (Aktion::Spur("Funk Nordic Star"), include_bytes!("../assets/11_funk_nordic_star.png")),
    (Aktion::Spur("Funk Bremen Rescue"), include_bytes!("../assets/12_funk_bremen_rescue.png")),
    (Aktion::Rueckgaengig, include_bytes!("../assets/15_rueckgaengig.png")),
];

const WEISS: Rgba<u8> = Rgba([255, 255, 255, 255]);
const ROT: Rgba<u8> = Rgba([230, 20, 20, 255]);

// ---------------------------------------------------------------------------
// Audacity-Pipe
// ---------------------------------------------------------------------------

struct Audacity {
    to: File,
    from: BufReader<File>,
}

impl Audacity {
    fn verbinden() -> io::Result<Audacity> {
        let uid = unsafe { libc::getuid() };
        let to_pfad = format!("/tmp/audacity_script_pipe.to.{uid}");
        let from_pfad = format!("/tmp/audacity_script_pipe.from.{uid}");

        // Nicht blockierend öffnen: Läuft Audacity nicht, kommt sofort ENXIO statt ewig zu hängen.
        let to = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&to_pfad)?;
        unsafe {
            let fd = to.as_raw_fd();
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
        // Audacity öffnet die Antwort-Pipe, sobald wir verbunden sind.
        let from = File::open(&from_pfad)?;
        Ok(Audacity { to, from: BufReader::new(from) })
    }

    fn befehl(&mut self, cmd: &str) -> io::Result<Vec<String>> {
        self.to.write_all(cmd.as_bytes())?;
        self.to.write_all(b"\n")?;
        self.to.flush()?;
        let mut zeilen = Vec::new();
        loop {
            let mut zeile = String::new();
            let n = self.from.read_line(&mut zeile)?;
            if n == 0 {
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "Audacity hat die Verbindung beendet"));
            }
            if zeile == "\n" && !zeilen.is_empty() {
                break;
            }
            zeilen.push(zeile.trim_end_matches('\n').to_string());
        }
        if let Some(letzte) = zeilen.last() {
            if letzte.starts_with("BatchCommand finished") && !letzte.ends_with("OK") {
                eprintln!("Audacity meldet: {letzte}  (Befehl: {cmd})");
            }
        }
        Ok(zeilen)
    }

    fn spuren(&mut self) -> io::Result<Vec<String>> {
        let antwort = self.befehl("GetInfo: Type=Tracks Format=JSON")?;
        let json: String = antwort
            .iter()
            .filter(|z| !z.starts_with("BatchCommand finished"))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let wert: serde_json::Value = serde_json::from_str(&json)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("Spurliste unlesbar: {e}")))?;
        Ok(wert
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|t| t.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Legt alle fehlenden Spuren an. Vorhandene bleiben unangetastet.
    fn spuren_anlegen(&mut self) -> io::Result<usize> {
        let vorhanden = self.spuren()?;
        let mut neu = 0;
        for name in SPUREN {
            if vorhanden.iter().any(|v| v.eq_ignore_ascii_case(name)) {
                continue;
            }
            self.befehl("NewMonoTrack:")?;
            let anzahl = self.spuren()?.len();
            self.befehl(&format!("SelectTracks: Track={} TrackCount=1 Mode=Set", anzahl - 1))?;
            self.befehl(&format!("SetTrackStatus: Name=\"{name}\""))?;
            neu += 1;
        }
        if neu > 0 {
            self.befehl("SelectNone:")?;
        }
        Ok(neu)
    }

    fn spur_waehlen(&mut self, name: &str) -> io::Result<bool> {
        let spuren = self.spuren()?;
        let Some(i) = spuren.iter().position(|s| s.trim().eq_ignore_ascii_case(name)) else {
            return Ok(false);
        };
        self.befehl(&format!("SelectTracks: Track={i} TrackCount=1 Mode=Set"))?;
        self.befehl("SetTrackStatus: Focused=1")?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Bilder
// ---------------------------------------------------------------------------

fn mit_rahmen(bild: &DynamicImage, farbe: Rgba<u8>, dicke: u32) -> DynamicImage {
    let mut img: RgbaImage = bild.to_rgba8();
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            if x < dicke || y < dicke || x >= w - dicke || y >= h - dicke {
                img.put_pixel(x, y, farbe);
            }
        }
    }
    DynamicImage::ImageRgba8(img)
}

fn abgedunkelt(bild: &DynamicImage) -> DynamicImage {
    let mut img = bild.to_rgba8();
    for p in img.pixels_mut() {
        for c in 0..3 {
            p[c] = (p[c] as u16 * 35 / 100) as u8;
        }
    }
    DynamicImage::ImageRgba8(img)
}

struct Tastenbilder {
    normal: DynamicImage,
    aktiv: DynamicImage,
    fehler: DynamicImage,
    aus: DynamicImage,
}

fn bilder_laden() -> Vec<Tastenbilder> {
    BELEGUNG
        .iter()
        .map(|(_, bytes)| {
            let normal = image::load_from_memory(bytes).expect("eingebettetes Bild defekt");
            Tastenbilder {
                aktiv: mit_rahmen(&normal, WEISS, 12),
                fehler: mit_rahmen(&normal, ROT, 16),
                aus: abgedunkelt(&normal),
                normal,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Zustand und Hauptschleife
// ---------------------------------------------------------------------------

static BEENDEN: AtomicBool = AtomicBool::new(false);

extern "C" fn signal_handler(_: libc::c_int) {
    BEENDEN.store(true, Ordering::SeqCst);
}

struct Zustand {
    aktive_spur: Option<&'static str>,
    nimmt_auf: bool,
    blink_an: bool,
    fehler_bis: Option<(usize, Instant)>,
    audacity_da: bool,
}

/// Deck-Tastenindex -> Index in BELEGUNG
fn zuordnung(deck: &StreamDeck) -> Vec<Option<usize>> {
    let kind = deck.kind();
    let spalten = kind.column_count() as usize;
    let anzahl = kind.key_count() as usize;
    (0..anzahl)
        .map(|taste| {
            let (zeile, spalte) = (taste / spalten, taste % spalten);
            if spalte >= SPALTEN {
                return None;
            }
            let i = zeile * SPALTEN + spalte;
            (i < BELEGUNG.len()).then_some(i)
        })
        .collect()
}

fn zeichnen(deck: &StreamDeck, map: &[Option<usize>], bilder: &[Tastenbilder], z: &Zustand) -> Result<(), elgato_streamdeck::StreamDeckError> {
    for (taste, eintrag) in map.iter().enumerate() {
        let Some(i) = *eintrag else {
            deck.clear_button_image(taste as u8)?;
            continue;
        };
        let b = &bilder[i];
        let aktion = BELEGUNG[i].0;
        let bild = if matches!(z.fehler_bis, Some((f, _)) if f == i) {
            &b.fehler
        } else if !z.audacity_da {
            &b.aus
        } else {
            match aktion {
                Aktion::Spur(name) if z.aktive_spur == Some(name) => &b.aktiv,
                Aktion::Aufnahme if z.nimmt_auf && z.blink_an => &b.fehler,
                _ => &b.normal,
            }
        };
        deck.set_button_image(taste as u8, bild.clone())?;
    }
    deck.flush()
}

/// Verbindet mit Audacity und legt fehlende Spuren an.
fn audacity_einrichten() -> io::Result<Audacity> {
    let mut a = Audacity::verbinden()?;
    println!("Mit Audacity verbunden.");
    match a.spuren_anlegen() {
        Ok(n) if n > 0 => println!("{n} Spur(en) angelegt."),
        Ok(_) => {}
        Err(e) => eprintln!("Spuren anlegen fehlgeschlagen: {e}"),
    }
    Ok(a)
}

fn ausfuehren(audacity: &mut Option<Audacity>, z: &mut Zustand, aktion: Aktion) -> bool {
    if audacity.is_none() {
        match audacity_einrichten() {
            Ok(a) => *audacity = Some(a),
            Err(e) => {
                eprintln!("Audacity nicht erreichbar ({e}). Läuft Audacity, ist mod-script-pipe aktiv?");
                z.audacity_da = false;
                return false;
            }
        }
    }
    z.audacity_da = true;
    let a = audacity.as_mut().unwrap();
    let ergebnis = match aktion {
        Aktion::Spur(name) => a.spur_waehlen(name).map(|ok| {
            if ok {
                z.aktive_spur = Some(name);
            } else {
                eprintln!("Spur \"{name}\" nicht gefunden.");
            }
            ok
        }),
        Aktion::Aufnahme => {
            if z.nimmt_auf {
                a.befehl("Stop:").map(|_| {
                    z.nimmt_auf = false;
                    true
                })
            } else {
                a.befehl("Record1stChoice:").map(|_| {
                    z.nimmt_auf = true;
                    true
                })
            }
        }
        Aktion::Stopp => a.befehl("Stop:").map(|_| {
            z.nimmt_auf = false;
            true
        }),
        Aktion::Rueckgaengig => a.befehl("Undo:").map(|_| true),
    };
    match ergebnis {
        Ok(ok) => ok,
        Err(e) => {
            eprintln!("Verbindung zu Audacity verloren: {e}");
            *audacity = None;
            z.audacity_da = false;
            z.nimmt_auf = false;
            false
        }
    }
}

fn deck_finden() -> Option<StreamDeck> {
    let hid = match new_hidapi() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("HID-Zugriff fehlgeschlagen: {e}");
            return None;
        }
    };
    let geraete = list_devices(&hid);
    let (kind, serial) = geraete.into_iter().next()?;
    match StreamDeck::connect(&hid, kind, &serial) {
        Ok(d) => {
            println!("Stream Deck gefunden: {kind:?} (Seriennummer {serial}), {} Tasten.", kind.key_count());
            Some(d)
        }
        Err(e) => {
            eprintln!(
                "Stream Deck gefunden, aber Öffnen fehlgeschlagen: {e}\n\
                 → udev-Regel installiert? Läuft noch StreamController (bitte beenden)?"
            );
            None
        }
    }
}

fn helligkeit_aus_args() -> u8 {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--helligkeit")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(70)
        .min(100)
}

fn nur_setup() {
    match Audacity::verbinden() {
        Ok(mut a) => match a.spuren_anlegen() {
            Ok(n) => println!("Fertig. {n} Spur(en) neu angelegt."),
            Err(e) => eprintln!("Fehler: {e}"),
        },
        Err(e) => eprintln!("Audacity nicht erreichbar: {e}"),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "hoerspiel-deck – Stream Deck steuert Audacity\n\n\
             Aufruf:\n  hoerspiel-deck [--helligkeit 0-100]   Deck starten (Strg+C beendet)\n  \
             hoerspiel-deck setup                  nur die Spuren in Audacity anlegen"
        );
        return;
    }
    if args.get(1).map(|s| s.as_str()) == Some("setup") {
        nur_setup();
        return;
    }

    unsafe {
        libc::signal(libc::SIGINT, signal_handler as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, signal_handler as *const () as libc::sighandler_t);
    }

    let helligkeit = helligkeit_aus_args();
    let bilder = bilder_laden();
    let mut audacity: Option<Audacity> = None;
    let mut z = Zustand { aktive_spur: None, nimmt_auf: false, blink_an: false, fehler_bis: None, audacity_da: false };

    println!("hoerspiel-deck läuft. Beenden mit Strg+C.");
    let mut hinweis_gezeigt = false;

    'aussen: while !BEENDEN.load(Ordering::SeqCst) {
        let Some(deck) = deck_finden() else {
            if !hinweis_gezeigt {
                println!("Warte auf Stream Deck …");
                hinweis_gezeigt = true;
            }
            sleep(Duration::from_secs(2));
            continue;
        };
        hinweis_gezeigt = false;
        let deck = Arc::new(deck);
        let map = zuordnung(&deck);
        if map.iter().filter(|m| m.is_some()).count() < BELEGUNG.len() {
            eprintln!("Hinweis: Dieses Deck hat weniger Tasten als die Belegung, nicht alle Funktionen sind erreichbar.");
        }
        let _ = deck.reset();
        let _ = deck.set_brightness(helligkeit);

        // Erster Verbindungsversuch zu Audacity, damit die Tasten gleich richtig aussehen.
        if audacity.is_none() {
            if let Ok(a) = audacity_einrichten() {
                audacity = Some(a);
                z.audacity_da = true;
            } else {
                println!("Audacity noch nicht erreichbar, Tasten sind abgedunkelt. Ein Tastendruck verbindet neu.");
            }
        }

        if let Err(e) = zeichnen(&deck, &map, &bilder, &z) {
            eprintln!("Deck-Fehler: {e}");
            continue;
        }

        let leser = deck.get_reader();
        let mut letztes_blinken = Instant::now();
        let mut letzter_check = Instant::now();

        loop {
            if BEENDEN.load(Ordering::SeqCst) {
                // Aufräumen: Deck leeren, damit keine veralteten Bilder stehen bleiben.
                let _ = deck.clear_all_button_images();
                let _ = deck.flush();
                let _ = deck.set_brightness(0);
                break 'aussen;
            }
            let updates = match leser.read(Some(Duration::from_millis(100))) {
                Ok(u) => u,
                Err(e) => {
                    eprintln!("Stream Deck getrennt ({e}). Suche erneut …");
                    sleep(Duration::from_secs(1));
                    continue 'aussen;
                }
            };

            let mut neu_zeichnen = false;
            for u in updates {
                if let DeviceStateUpdate::ButtonDown(taste) = u {
                    if let Some(Some(i)) = map.get(taste as usize) {
                        let aktion = BELEGUNG[*i].0;
                        if !ausfuehren(&mut audacity, &mut z, aktion) {
                            z.fehler_bis = Some((*i, Instant::now() + Duration::from_millis(800)));
                        }
                        neu_zeichnen = true;
                    }
                }
            }

            if let Some((_, bis)) = z.fehler_bis {
                if Instant::now() >= bis {
                    z.fehler_bis = None;
                    neu_zeichnen = true;
                }
            }
            if z.nimmt_auf && letztes_blinken.elapsed() >= Duration::from_millis(500) {
                z.blink_an = !z.blink_an;
                letztes_blinken = Instant::now();
                neu_zeichnen = true;
            }
            // Abgedunkelte Tasten wieder aufhellen, sobald Audacity startet.
            if audacity.is_none() && letzter_check.elapsed() >= Duration::from_secs(3) {
                letzter_check = Instant::now();
                if let Ok(a) = audacity_einrichten() {
                    audacity = Some(a);
                    z.audacity_da = true;
                    neu_zeichnen = true;
                }
            }

            if neu_zeichnen {
                if let Err(e) = zeichnen(&deck, &map, &bilder, &z) {
                    eprintln!("Deck-Fehler: {e}");
                    continue 'aussen;
                }
            }
        }
    }

    println!("Beendet.");
}
