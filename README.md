# QRDrop

Dateien zwischen PC und Handy austauschen, ohne Kabel, Account oder Cloud. Das Handy braucht nur den Browser.

## Benutzung

1. `QRDrop.exe` starten (Windows, 64-bit). Es öffnet sich ein Fenster mit einem QR-Code.
2. Mit dem Handy scannen. Handy und PC müssen im selben WLAN sein.
3. **Handy → PC:** Auf der Seite „Dateien senden“ tippen. Die Dateien landen in `Downloads\QRDrop`.
4. **PC → Handy:** Dateien auf das App-Fenster ziehen. Sie erscheinen auf der Handy-Seite unter „Vom PC“.

Beim ersten Start fragt die Windows-Firewall, ob das Programm erreichbar sein darf. „Privates Netzwerk“ muss erlaubt werden.

## Hinweise

- Die Adresse enthält einen zufälligen Token, der bei jedem Start neu erzeugt wird. Nur wer den QR-Code hat, kommt auf die Seite.
- Der Server läuft nur, solange das Fenster offen ist.
- In Gast-/Besuchernetzen sind Geräte oft voneinander getrennt. Dann hilft ein Handy-Hotspot oder der Windows-Hotspot.
- Windows SmartScreen kann warnen, weil die EXE nicht signiert ist: „Weitere Informationen“ → „Trotzdem ausführen“.

## Selbst bauen

```
cargo build --release
```

Die EXE liegt danach in `target\release\handy-transfer.exe`. Gebaut mit Rust, axum und egui.
