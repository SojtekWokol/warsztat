# ea1 – Escape Academy (część 1)

Spolszczenie części 1: wyciąganie tekstów, wgrywanie polskiego jako piątego języka, adapter warsztatu.

## Projekt i warsztat

Projekt EA1 to folder `ea1/` obok tłumaczenia EA2 (w repozytorium tłumaczeń trafia tylko
`ea1/translations/pl`):

- `ea1/translations/pl/**.json` – magazyn tłumaczeń (ten sam format co EA2): `scenki/<pokój>.json`,
  `rozmowy.json`, `pokoje/<pokój>.json`, `kampus.json`, `interfejs.json`, `napisy.json`, `nazwy.json`;
- `ea1/extracted/` – teksty gry (robione przy pierwszym starcie z czystych plików gry);
- `ea1/backup/<id instalacji>/` – kopie oryginałów (`kopie`, znacznik `Escape Academy_Data/ea1_pl.json`).

Identyfikatory: `g:<klucz GameplayStrings>`, `y:<id kwestii Yarn>` (kwestia bez przedrostka „Postać:”,
który w grze zostaje angielski, bo gra po nim wybiera mówiącą postać), `v:<klip>` (napis do nagrania –
jeszcze nie wgrywany).

`rust/target/release/warsztat-ea1.exe` – warsztat (interfejs skopiowany z EA2); „Wgraj do gry” robi to samo
co `ea1 patch`, „Uruchom grę” odpala `Escape Academy.exe`.

## Gra

- Wersja Epic: `C:\Users\grejs\Epic Games\EscapeAcademyOcy7i` (Unity 2020.3.40f1, Mono).
- Dane w paczce UnityFS `Escape Academy_Data\data.unity3d` (2,7 GB, 166 węzłów, 88 plików
  serializowanych, format 22, **bez type trees** – układ pól MonoBehaviourów trzeba znać) oraz w paczce
  Addressables `StreamingAssets\aa\StandaloneWindows64\areadioramas_assets_all_*.bundle` (1,1 GB,
  **z type trees**): kampus, 79 programów Yarn (rozmowy `MG_*`), `LanguageManager`, napisy końcowe.
- Języki w grze: angielski, francuski, niemiecki, hiszpański.

## Polecenia

```
ea1 extract   [--game FOLDER] [--out FOLDER]   # teksty do extracted_ea1/ (domyślnie w katalogu repo)
ea1 fonts     [--game FOLDER]                  # fonty TMP i źródłowe: czy mają polskie litery, kto ich używa
ea1 scan      [--game FOLDER]                  # które skrypty MonoBehaviour mają czytelne napisy (heurystyka)
ea1 tmpscan   [--game FOLDER]                  # komponenty TextMeshPro: układ, klucze, sąsiednie skrypty
ea1 roundtrip [--game FOLDER]                  # czy każdy plik zapisuje się z powrotem bajt w bajt
ea1 scripts   [FILTR]                          # nazwy klas wszystkich MonoScriptów
ea1 dump      FILTR                            # surowe dane MonoBehaviourów danej klasy (hex)
ea1 find      TEKST                            # obiekty, których dane zawierają tekst
ea1 patch     [--dry-run] [--out FOLDER]      # polski z ea1/translations do gry (kopie w ea1/backup/)
ea1 restore                                    # oryginalne pliki gry z kopii
ea1 testlang  --out FOLDER                     # wersja testowa polskiego jako 5. języka ([PL] + angielski)
ea1 ilcheck                                    # stan łatki IL w Assembly-CSharp.dll (Original/Patched)
ea1 crc       PACZKA                           # CRC32 paczki, jak w katalogu Addressables
```

## Gdzie są teksty

| Źródło | Co to | Wpisów (EN) |
|---|---|---|
| `GameplayStrings`, `_Flashback`, `_Island`, `_ProcGen` (TextAsset, CSV) | interfejs, przedmioty, podpowiedzi, napisy w pokojach; kolumny `Key, Value-En, Area/Context, French, German, Spanish` | 6369 (ok. 31 800 słów) |
| tablice Yarn (TextAsset CSV `id,text,file,node,lineNumber`), 138 programów `YarnProgram` | dialogi w przerywnikach i rozmowy na kampusie, `Postać:tekst` | 1152 |
| napisy do nagrań (TextAsset `.srt`, nazwa = klip, `_de/_fr/_es` dla innych języków) | okrzyki i kwestie podczas gry, `[POSTAĆ] tekst` | 1314 (ok. 10 600 słów) |
| `m_text` w komponentach TextMeshPro (sceny, prefaby) | 1293 różne odnośniki `@klucz` do GameplayStrings + 602 teksty wpisane na sztywno (napisy końcowe, znaki do zagadek, zaślepki) | – |

Tłumaczenie TMP działa tak, że tekst `@klucz` gra podmienia na wpis z GameplayStrings – wystarczy więc
przetłumaczyć tabele. 23 odnośniki nie mają wpisu w tabelach (lista w `summary.json`).
`GameplayStrings_Island` ma 2 bajty spoza UTF-8 (wielokropek z Windows-1252) – przy zapisie trzeba
oryginał traktować jako bajty.

Pliki w `extracted_ea1/` (lokalne, nie trafiają do repozytorium – jak `extracted/` z EA2):
`GameplayStrings*.txt` (surowe CSV), `yarn_<język>.json`, `yarn_programs.json` (tablice i węzły
programów), `yarn_tables_extra.json`, `subtitles_<język>.json`, `tmp_texts.json`, `summary.json`.

## Układy pól (bez type trees)

Wybrałem ręczne układy zamiast czytania metadanych .NET z `Managed\*.dll`: potrzebne są tylko
TextAsset (klasa wbudowana), `YarnProgram` (YarnSpinner 1.x: `compiledProgram`,
`baseLocalisationStringTable`, `baseLocalizationId`, `localizations[]`), początek `TMP_Text`
(do `m_text` i `m_fontAsset`) i nagłówek `TMP_FontAsset`. Każdy układ jest sprawdzany (koniec danych,
wskaźnik na font), a parser metadanych CLI z dziedziczeniem między bibliotekami to kilkaset linii
bez zysku na dziś. Wróćmy do tego, jeśli trzeba będzie czytać wiele innych skryptów.

## Fonty

Fonty TMP są statyczne i nie mają polskich liter (poza Ó/ó). `testlang` dorysowuje je patcherem z EA2
(`ea2::fonts`, lista fontów EA1 w `testlang::FONT_SOURCES`, pliki z `fonts/` w repo). Type trees dla
`TMP_FontAsset`/`Texture2D`/`Material` bierze z paczki Addressables, a te same drzewa obsługują
`data.unity3d`. Fonty są w obu paczkach (osobne kopie atlasów).

| Font asset | Plik | Komponentów | Stan |
|---|---|---|---|
| Urbanist | Urbanist-ExtraBold.ttf (wagę sprawdzono po szerokościach liter) | 5572 | dorysowane |
| Urbanist_body | Urbanist-SemiBold.ttf | 288 | dorysowane |
| ChunkFive | ChunkFive-Regular.ttf | 2046 | dorysowane |
| Kalam-Bold SDF | Kalam-Bold.ttf | 285 | dorysowane |
| knewave SDF | Knewave-PL.ttf | 191 | dorysowane |
| Contra SDF | brak pliku | 1434 | do zrobienia |
| college SDF | brak (76 znaków, chyba same wielkie litery) | 994 | do sprawdzenia |
| DS-DIGI SDF | brak (zegary, cyfry) | 214 | raczej niepotrzebne |
| RockSalt-Regular SDF | brak (Google Fonts, OFL) | 53 | do zrobienia |
| DOTMATRI SDF | brak | 32 | do sprawdzenia |

`LiberationSans SDF` jest dynamiczny, ale w grze i tak pokazywał kratki – może też wymagać dorysowania.

## Wybór języka (kod gry)

`Escape.Localization.LanguageManager` (w paczce Addressables) ma listę `languages` w danych:

| type (enum `LanguageType`) | nazwa | kod | csvIndex |
|---|---|---|---|
| 0 English | English | en | 1 |
| 1 Spanish | Español | es | 6 |
| 2 German | Deutsch | de | 5 |
| 3 French | Français | fr | 4 |

Co od czego zależy (z metadanych i wywołań w `Assembly-CSharp.dll`):

- menu i zapis ustawień: `languageCode` i `languageDisplayName` z listy – wpis da się dodać w danych;
- GameplayStrings (a więc wszystkie `@klucze` TMP): `StringReplacementManager` czyta kolumnę
  `csvIndex` – nowa kolumna 7 działa bez zmian w kodzie;
- dialogi Yarn: `VisualNovelManager.escapeLanguageToYarn` to słownik w kodzie
  (EN→en-US, ES→es, DE→de, FR→fr) wybierany po **enumie**; nieznany enum = tablica bazowa (angielska);
- napisy do nagrań: `VoSubtitleData` – po **enumie** (`localizedSubtitles[].language`), inaczej angielskie;
- grafiki i tekstury z napisami: `LocalizeSprite`, `LocalizedChairGraphic`, `LocalizeGameObjects` po
  enumie z powrotem do domyślnej; `LocalizedMaterials` używa `First()` – dla nieznanego enuma rzuci wyjątek
  w obsłudze `OnLanguageSet` (może przerwać pozostałe obsługi zmiany języka).

Enum ma tylko 4 wartości, więc gra sama z siebie nie obsłuży piątego języka.

## Polski jako piąty język (wybrana droga)

Polski dostaje w `languages` wpis `{type: 0 (English), "Polski", "pl", csvIndex 7}`. Wszystko, co idzie
po enumie (grafiki, materiały, kolory krzeseł, odpowiedzi zagadek zależne od języka, format telefonu),
zachowuje się jak po angielsku. Tak ma być, bo grafik nie tłumaczymy. Dwie małe łatki IL w
`Assembly-CSharp.dll` (moduł `il.rs`) rozróżniają polski i angielski tam, gdzie to potrzebne. Obie są
w miejscu, z `nop`, bez zmian w metadanych, i dopasowane do dokładnych bajtów oryginału:

1. `VisualNovelManager.InitializeVisualNovel`: język Yarn = `currentLanguage.languageCode` zamiast słownika
   enum→kod (dla en/es/fr/de wartości są te same, dla polskiego „pl”). YarnSpinner bierze wtedy
   `YarnTranslation` o `languageName == "pl"`, a gdy go brak, tablicę bazową.
2. `LanguageManager.get_IsCurrentLanguageDefault`: `csvIndex == 1` zamiast porównania enumów, więc dla
   polskiego gra szuka napisów `<klip>_pl` (`SubtitleDisplayer`) i wpisów `VoSubtitleData`, a gdy ich brak,
   pokazuje angielskie.

Dane:
- `LanguageManager` (paczka Addressables): nowy wpis;
- GameplayStrings: kolumna 7 „Polish”;
- każdy `YarnProgram` (185 w obu paczkach): `YarnTranslation {"pl", TextAsset "<program> (pl)"}`;
- paczka Addressables ma CRC w `catalog.json` (CRC32 nieskompresowanych danych, sprawdzone na
  oryginale): podmieniamy liczbę z dopełnieniem spacjami, żeby długość JSON-u się nie zmieniła.

`ea1 testlang --out FOLDER [--pl PLIK]` buduje to wszystko z angielskimi tekstami oznaczonymi „[PL] ” (albo gotowymi polskimi kwestiami Yarn z pliku JSON `{id kwestii: tekst}`) (pliki w układzie
folderu gry, gry nie dotyka). Sprawdzone poza grą: `ilcheck` = Patched, `extract` z wyniku widzi
5 języków, kolumnę Polish i 1152 kwestie `pl`, UnityPy czyta przepakowaną paczkę Addressables i widzi
tłumaczenia `pl`. **Nie sprawdzone w grze.**

Zostaje do zrobienia:
- napisy do nagrań: nowe TextAssety `<klip>_pl` w `resources.assets` + wpisy w kontenerze
  `ResourceManager` (Resources.Load) oraz wpisy `{0, pl}` w `VoSubtitleData`;
- fonty Contra i RockSalt (brak plików źródłowych), sprawdzić college/DS-DIGI/DOTMATRI i LiberationSans;
- polskie imiona mówiących (przedrostek „Postać:” w Yarn) – sprawdzić, czy gra dobiera po nim postać;
- teksty wpisane w komponenty TMP (napisy końcowe) i wykrywanie polskiego systemu
  (`SystemLanguageToLanguageType` w kodzie) – na razie polski wybiera się w menu, a gra zapamiętuje kod `pl`;
- wspólny interfejs warsztatu dla EA1 i EA2 (dziś `warsztat-ea1/ui` to kopia `warsztat-ea2/ui`).
