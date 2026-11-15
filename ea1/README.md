# ea1 – Escape Academy (część 1)

Pierwszy krok do spolszczenia części 1: wyciąganie tekstów. Wgrywania jeszcze nie ma.

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

Główne fonty TMP są statyczne i nie mają polskich liter (poza Ó/ó): Urbanist (5572 komponenty),
ChunkFive (2046), Contra (1434), college (994), Kalam-Bold, Urbanist_body, DS-DIGI, knewave,
RockSalt, DOTMATRI. Dynamiczny jest tylko `LiberationSans SDF` (593 komponenty) – jego font źródłowy
ma polskie litery, więc tam zadziała od razu. Trzeba dorysować glify SDF jak w EA2
(`ea2::fonts`/`ea2::sdf`; Urbanist jest w obu grach).

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

`ea1 testlang --out FOLDER` buduje to wszystko z angielskimi tekstami oznaczonymi „[PL] ” (pliki w układzie
folderu gry, gry nie dotyka). Sprawdzone poza grą: `ilcheck` = Patched, `extract` z wyniku widzi
5 języków, kolumnę Polish i 1152 kwestie `pl`, UnityPy czyta przepakowaną paczkę Addressables i widzi
tłumaczenia `pl`. **Nie sprawdzone w grze.**

Zostaje do zrobienia:
- napisy do nagrań: nowe TextAssety `<klip>_pl` w `resources.assets` + wpisy w kontenerze
  `ResourceManager` (Resources.Load) oraz wpisy `{0, pl}` w `VoSubtitleData`;
- polskie litery w fontach SDF (jak w EA2);
- `ea1 patch/restore` z kopiami oryginałów (`kopie`), magazyn tłumaczeń i `warsztat-ea1`;
- wykrywanie polskiego systemu (słownik `SystemLanguageToLanguageType` w kodzie) – na razie polski
  wybiera się w menu, a gra zapamiętuje kod `pl`.

## Plan wgrywania

To samo co w EA2 dla `data.unity3d`: podmienić TextAssety (CSV z kolumną polską albo nadpisaną
kolumną języka), zapisać pliki serializowane i przepakować paczkę. Zapis jest gotowy: `roundtrip`
daje 88/88 plików identycznych. Do ustalenia: jak gra wybiera język (kolumna CSV, `LanguageManager`)
– czy da się dodać polski jako piąty język, czy podmieniamy np. hiszpański.
Propozycja: crate `ea1` jak `ea2` (`extract`, `patch`, `restore`, `fonts` ze wspólnym kodem SDF
przeniesionym do osobnego crate'a) i `warsztat-ea1` z implementacją `Game` z `warsztat-base`.
