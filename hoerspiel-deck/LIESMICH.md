# hoerspiel-deck (Rust)

Eigenständiges Programm: spricht das Stream Deck direkt über USB an, setzt die Tastenbilder selbst und steuert Audacity über `mod-script-pipe`. StreamController wird nicht mehr gebraucht. Er muss sogar **beendet** sein, weil immer nur ein Programm das Deck öffnen kann.

## Was es macht
- Findet das Deck, setzt Helligkeit und alle 15 Tastenbilder (die Bilder stecken im Programm).
- Verbindet sich mit Audacity und legt fehlende Spuren automatisch an. Vorhandene Spuren bleiben unangetastet.
- Sprecher-Taste wählt genau diese Spur. Die **aktive Spur bekommt einen weißen Rahmen.**
- AUFNAHME startet die Aufnahme, ein zweiter Druck stoppt sie. **Während der Aufnahme blinkt die Taste rot.**
- STOPP, ZURÜCK (Rückgängig).
- Ist Audacity nicht gestartet, sind alle Tasten **abgedunkelt**. Sobald Audacity läuft, werden sie automatisch wieder hell.
- Wenn etwas schiefgeht (zum Beispiel eine Spur fehlt), leuchtet die gedrückte Taste kurz rot. Details stehen im Terminal.
- Deck abziehen oder Audacity neu starten ist kein Problem, das Programm verbindet sich selbst neu.
- Strg+C beendet das Programm und macht das Deck dunkel.

## Belegung (5 × 3)

| Erzähler     | Hinnerk      | Jonas            | Birte              | Ole    |
|--------------|--------------|------------------|--------------------|--------|
| Klaas        | Nordic Star  | Bremen Rescue    | AUFNAHME           | STOPP  |
| Funk Hinnerk | Funk Klaas   | Funk Nordic Star | Funk Bremen Rescue | ZURÜCK |

## Einrichten (einmalig)

1. **USB-Rechte** für das Deck:

       sudo cp extras/70-streamdeck.rules /etc/udev/rules.d/
       sudo udevadm control --reload && sudo udevadm trigger

   Danach das Deck einmal ab- und wieder anstecken.

2. **Audacity**: als apt-Paket oder AppImage, nicht als Snap/Flatpak, denn die haben ein eigenes /tmp.
   Bearbeiten → Einstellungen → Module → `mod-script-pipe` = Aktiviert, dann Audacity neu starten.

3. **Programm holen**: Auf GitHub unter *Actions → Hörspiel-Deck bauen* den letzten Lauf öffnen und das Artefakt `hoerspiel-deck` herunterladen (gebaut für Ubuntu 24.04 oder neuer, x86_64). Alternativ selbst bauen, siehe unten. Dann:

       mkdir -p ~/.local/bin
       cp hoerspiel-deck ~/.local/bin/
       chmod +x ~/.local/bin/hoerspiel-deck

## Benutzen

    hoerspiel-deck                     # starten
    hoerspiel-deck --helligkeit 40     # dunkler
    hoerspiel-deck setup               # nur die Spuren in Audacity anlegen

### Optional: automatisch beim Anmelden starten

    mkdir -p ~/.config/systemd/user
    cp extras/hoerspiel-deck.service ~/.config/systemd/user/
    systemctl --user enable --now hoerspiel-deck

Ausgaben ansehen mit `journalctl --user -u hoerspiel-deck -f`.

## Selbst bauen

    sudo apt install build-essential pkg-config libudev-dev
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh     # falls Rust fehlt
    cd hoerspiel-deck
    cargo build --release
    # Ergebnis: target/release/hoerspiel-deck

## Anpassen
Alles steht oben in `src/main.rs`:
- `SPUREN`: welche Spuren angelegt werden.
- `BELEGUNG`: welche Taste was tut, und welches Bild. Die Bilder liegen in `assets/` (144 × 144 PNG) und werden beim Bauen eingebettet.

## Hinweis zur Aufnahme
AUFNAHME entspricht der Taste R in Audacity. Ob Audacity an der Cursorposition aufnimmt oder ans Spurende anhängt, hängt von Version und Einstellungen ab. Teste es einmal mit zwei Spuren.
