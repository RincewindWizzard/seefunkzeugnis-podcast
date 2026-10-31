# SRC-Lernskript

Hörspiel-Drehbuch zum Lernen der Funkspruchtypen für das SRC (Short Range Certificate).

- `SRC-Lernskript.md` – das Drehbuch. Hier wird bearbeitet.
- `build/convert.py` – baut daraus die HTML-Seite.
- `build/template.html` – Layout, Stil, Rollenfilter, Auto-Scroll.
- `.github/workflows/pages.yml` – veröffentlicht bei jedem Push auf `main` automatisch als GitHub Page.
- `hoerspiel-deck/` – Rust-Programm: Stream Deck steuert Audacity beim Einsprechen (eine Taste pro Sprecher- und Funkspur). Anleitung in `hoerspiel-deck/LIESMICH.md`.

Lokal bauen:

    python build/convert.py SRC-Lernskript.md build/template.html _site/index.html

## Format im Skript

- `# Szene …` / `## 9a: …` – Überschriften
- `Rollenname:` am Zeilenanfang – danach eingerückt (Tab) der Text der Rolle
- eingerücktes `Am Funkgerät:` – Geräteeinstellungen bis zur nächsten Leerzeile
- Zeilen zwischen `-----` – Funkspruch
- `*Platsch*` – Soundeffekt
- nicht eingerückte Zeilen ohne Rolle – Regieanweisung

Neue Rollen müssen in `build/convert.py` (ROLES), in der Figurentabelle und als Farbe in `build/template.html` eingetragen werden.
