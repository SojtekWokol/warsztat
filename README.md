# Warsztat tłumaczeń

Narzędzia do fanowskich tłumaczeń gier, w Rust: wspólny warsztat (edytor z podglądem, słownik, pamięć tłumaczeń,
podpowiedzi z lokalnego modelu Bielik, kontrola znaczników, poprawki z plików, wydania) oraz moduły konkretnych gier.

| Crate | Co robi |
|---|---|
| `unity` | odczyt i zapis paczek Unity (UnityFS, pliki serializowane, type trees) |
| `kopie` | kopie oryginalnych plików gry, bezpieczne przy aktualizacjach gry i między narzędziami (znacznik w folderze gry) |
| `warsztat-base` | wspólny rdzeń warsztatu: magazyn tłumaczeń, API dla interfejsu, kontrole, słownik, Bielik, poprawki, wydania |
| `ea2` | Escape Academy 2: wyciąganie tekstów z gry, wgrywanie tłumaczenia (teksty, dialogi Yarn, polskie litery w fontach SDF, katalog Addressables), adapter warsztatu |
| `warsztat-ea2` | aplikacja warsztatu EA2 (Tauri) |
| `instalator-base` | wspólny instalator dla graczy: okno, szukanie gry (Xbox, Steam), uprawnienia folderu, instalacja i przywracanie z postępem |
| `instalator` | instalator spolszczenia EA2: wtyczka do `instalator-base` z łatką, tłumaczeniem, fontami i wyglądem EA2 |

Nowa gra dokłada swoje crate'y: adapter warsztatu (`warsztat::Game`) i instalator (`instalator::Installer` + własny `theme.css`).
Wspólne części (edytor, kopie, okno instalatora) poprawia się raz, w jednym miejscu.

## Budowa

```
cargo build --release -p warsztat-ea2     # target/release/warsztat-ea2.exe
cargo build --release -p instalator       # target/release/ea2-spolszczenie.exe
```

Instalator wbudowuje tłumaczenie: ustaw `EA2_TRANSLATIONS` (folder `translations` repozytorium tłumaczenia),
`EA2_FONTS` (folder `fonts`) i opcjonalnie `EA2_RELEASE` (np. `1.0.0`).

Narzędzie wiersza poleceń `ea2`:

```
ea2 extract [--content FOLDER_GRY]        # teksty i obrazki z gry do extracted/ i cache/vn
ea2 patch   [--content FOLDER_GRY]        # wgranie tłumaczenia (kopie oryginałów w backup/)
ea2 restore [--content FOLDER_GRY]        # przywrócenie oryginałów
```

Wydanie warsztatu: tag `warsztat-vX.Y.Z` → GitHub Actions buduje `EA2 Warsztat.exe`.
