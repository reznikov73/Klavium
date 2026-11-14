# Klavium — передача работы другому агенту

Состояние на **2026-11-14**: версия **0.2.10**, ветка `main`, коммит `2391ba2`,
рабочая копия чистая, `origin/main` синхронизирован. Приложение собрано,
установлено и запущено.

Репозиторий: `E:\Work\GIGA\GitHub\Klavium`, remote `https://github.com/reznikov73/Klavium.git`.
Установленная копия: `E:\Program Files\Klavium\klavium.exe`, данные —
`E:\Program Files\Klavium\data` (`config.json`, `logs\app.log`, `models`, `user_dict.txt`).

---

## 1. Что это за проект

Фоновая Windows-утилита исправления раскладки (аналог Punto Switcher) на Rust + egui.
Ставит низкоуровневый хук клавиатуры, копит последнее слово, проверяет его словарями
и биграммами, при ошибке перепечатывает слово в правильной раскладке. Есть голосовой
ввод (Whisper/Vosk/Google), трей, статистика, оверлеи-подсказки.

Ведём работу по **своему ТЗ** — [docs/TZ-0.2.md](TZ-0.2.md). Оно написано после ревью
исходного ТЗ пользователя; разделы 1–6 (P0/P1: устойчивость, интерфейс, память, скорость)
закрыты в 0.1.8–0.2.2, раздел 7 (функции) — частично.

## 2. Обязательный цикл работы

```powershell
$env:LIBCLANG_PATH = "$env:USERPROFILE\.libclang\clang\native"

cargo fmt --all --check
cargo clippy --workspace --all-targets     # должно быть без предупреждений
cargo test --workspace                      # 51 + 101 тест, все зелёные
cargo build --release -p klavium-app
```

Затем:

```powershell
Stop-Process -Name klavium -Force -ErrorAction SilentlyContinue
Copy-Item target\release\klavium.exe 'E:\Program Files\Klavium\klavium.exe' -Force
Start-Process 'E:\Program Files\Klavium\klavium.exe'
pwsh -NoProfile -File tools\build_dist.ps1 -SkipBuild
git add -A; git commit -m "…"; git push origin main
```

Правила проекта:

- **Язык всего** — русский: ответы, комментарии в коде, сообщения коммитов, документация.
- Версия поднимается **на каждый инкремент** в корневом `Cargo.toml` (`version = "0.2.9"`).
- `WHISPER_DONT_GENERATE_BINDINGS=1` использовать **нельзя** — бандл-биндинги ломают
  сборку под Windows.
- `panic = "abort"` включать **нельзя**: код полагается на `PoisonError::into_inner`
  и раскрутку стека в тестах.
- `target/` и `dist/` в `.gitignore`; `Cargo.lock` версионируется (это приложение).

## 3. Что уже сделано (по версиям)

| Версия | Что |
|---|---|
| 0.1.8 | Устойчивость: детекция ушла с потока хука в воркер (`Job::Key`), `pending_errors_count` на атомике, канал `OnceLock`, пример `crates/klavium-app/examples/hook_budget.rs` (p99 3 мкс, max 1,1 мс) |
| 0.2.0 | Интерфейс 2.0: `klavium-ui/src/theme.rs` (токены), `widgets.rs` (card, setting_row, switch, keycap, badge, segmented, metric_card, color_pill), светлая тема, сайдбар, нижняя панель, все 8 секций, скриншоты |
| 0.2.1 | Память: FST-словари в `klavium-core/build.rs` (exe 17,85 → 12,69 МБ, RSS 81,5 → 13,5 МБ), `lto=fat`, `codegen-units=1`, `platform/src/mem.rs` + `spawn_memory_trim` |
| 0.2.2 | Скорость: `inject::paste_text`, `voice.paste_long_text`, `typing.speed_correction/speed_voice` (careful/fast/instant) |
| 0.2.3 | 7.1 Отмена последнего исправления `Ctrl+Alt+Z` (`LastCorrection`, `can_undo`) |
| 0.2.4 | 7.2 Исправление выделенного `Ctrl+Alt+X` (`inject::copy_selection`, `Job::FixSelection`); Ctrl+C/V одной пачкой `SendInput`; определение выделения через `GetClipboardSequenceNumber` |
| 0.2.5 | Отмена работает и для пакетной правки; в стартовую строку журнала добавлены отмена= и выделенное=; сторож хука: порог 5 с, предупреждение не чаще раза в 5 минут |
| 0.2.6 | 7.3 Пользовательский словарь `data/user_dict.txt` (`!слово` — стоп-лист, `#` — комментарий), кэш по mtime раз в 60 с, карточка «Словарь пользователя» |
| 0.2.7 | 7.9 Раздел «Диагностика»: версия, каталог данных, путь журнала, активные сочетания, состояние хука, последний колбэк, медленные колбэки, кнопки «Открыть журнал/папку», «Копировать отчёт» |
| 0.2.8 | 7.4 Правила для приложений (`correction.app_rules`, `off`/`manual`, `window::foreground_process_name()` с кэшем по HWND, карточка «Приложения», «Текущее окно») + 7.5 Пауза коррекции из трея (30 мин / до перезапуска / снять; только в памяти; тултип «(пауза)»; тост) |
| 0.2.9 | 7.6 Push-to-talk: `voice.mode = toggle | push`, автомат `voice::push::PushToTalk`, хук ловит key-up клавиши диктовки, `VoiceInbound::PushStart/PushStop`, бейдж «ГОВОРИТЕ» |
| 0.2.10 | 7.7 Транслитерация `Ctrl+Alt+T`: `convert::translit` (ГОСТ 7.79 B с бытовыми заменами), `detect_translit_direction`, `Job::Translit` — выделение через буфер и текущее набираемое слово, переключение раскладки под итоговый текст, тосты «было → стало» и причины пропуска, скрипт `tools\check-translit.ps1` |

## 4. Что осталось — следующий шаг

### 7.8. Сниппеты (следующее, версия 0.2.11)

- `snippets: Vec<Snippet { trigger, expansion }>` в конфиге; триггер ≥ 2 символов,
  без пробелов.
- Срабатывание по завершении слова, **приоритет выше коррекции**; вставка длинного —
  через `paste_text` (6.1).
- Приёмка: `;адр` → адрес; при конфликте с словарным словом побеждает сниппет; тесты.
- Нужна карточка редактирования в настройках (список + добавить/удалить), по образцу
  карточки «Приложения» в `draw_app_rules`.

### Затем — 0.3.0

README и свежие скриншоты всех секций (`docs/images`), разбор «что нового», проверка
всех 9 дымовых прогонов, тег релиза по желанию пользователя.

## 5. Как проверять функции (шпаргалка)

Дымовые прогоны секций (каждая должна выйти с кодом 0):

```powershell
$pages = 'correction','hotkeys','sounds','timeouts','notifications','statistics','general','voice','diagnostics'
foreach ($p in $pages) {
    $env:KLAVIUM_APP_SETTINGS_PAGE = $p; $env:KLAVIUM_APP_SMOKE = '5'
    $pr = Start-Process 'E:\Program Files\Klavium\klavium.exe' -PassThru
    $pr.WaitForExit(15000) | Out-Null
    if (-not $pr.HasExited) { Stop-Process -Id $pr.Id -Force; "$p TIMEOUT" }
    elseif ($pr.ExitCode -ne 0) { "$p $($pr.ExitCode)" } else { "$p ok" }
}
Remove-Item Env:KLAVIUM_APP_SETTINGS_PAGE, Env:KLAVIUM_APP_SMOKE
```

Важно: перед прогоном **закрыть все экземпляры** `klavium`, иначе снимок/окно возьмётся
от чужой копии.

Скрипты проверки (в `tools/`, запускать из корня репозитория):

| Скрипт | Назначение |
|---|---|
| `tools\stress-typing.ps1 -Words N` | печатает «ghbdtn » N раз скан-кодами в активное окно (Блокнот) |
| `tools\check-translit.ps1` | живая проверка транслитерации: своё поле WinForms, набор скан-кодами, `Ctrl+Alt+T` в обе стороны; код 0 — всё прошло |
| `tools\combo-scan.ps1` | нажимает хоткей исправления (`Ctrl+Shift+Z`) |
| `tools\combo-undo.ps1` | нажимает отмену (`Ctrl+Alt+Z`) |
| `tools\hold-combo.ps1 -Vk 0x4D -Scan 0x32 -HoldMs 2000` | удержание сочетания (push-to-talk); **без `-Scan` не сработает** для `Trigger::Char` |
| `tools\ui-shot-settings.ps1 -Out … [-Notches -6] [-Width 860 -Height 900]` | находит окно настроек по PID, скроллит, снимает PNG |
| `tools\ui-shot.ps1 -X -Y -W -H -Out` | снимок области экрана |
| `tools\ui-wheel.ps1 -Notches -6` | прокрутка колесом в позиции курсора (минус — вниз) |

Ручные проверки:

- **Исправление**: в Блокноте набрать `ghbdtn ` → карточка «было → станет» → `Ctrl+Shift+Z`.
- **Отмена**: после исправления `Ctrl+Alt+Z` → вернулись `ghbdtn` и EN.
- **Выделенное**: выделить `ghbdtn vbh`, нажать `Ctrl+Alt+X`.
- **Транслитерация**: набрать `привет` → `Ctrl+Alt+T` → `privet` (раскладка стала EN);
  набрать `privet` → `Ctrl+Alt+T` → `привет`. На выделении: любой текст + `Ctrl+Alt+T`.
  В журнале — `транслит слова: «привет» → «privet»` или `транслит: «…» → «…»`,
  при пустом вводе — `транслит: пропуск — нет ни выделения, ни слова` и тост.
- **Словарь**: `data\user_dict.txt` — строка `привет` не исправляется, `!слово` запрещает.
- **Правила приложений**: «Общие» → «Приложения» → «Текущее окно» (нажать и переключиться
  в Блокнот, через 3 с правило появится) → в Блокноте исправление не работает; режим
  «Только по хоткею» — `Ctrl+Shift+Z` всё ещё исправляет.
- **Пауза**: трей → «Пауза на 30 минут» → в трее «(пауза 30 мин)», исправление молчит;
  «Снять паузу» — снова работает; после перезапуска паузы нет.
- **Диагностика**: «Диагностика» → версия, каталог данных, сочетания, «хук получает
  клавиатуру», «Медленных колбэков: 0 — норма».
- **Push-to-talk**: «Голос» → «Удержание» → удерживать `Ctrl+Shift+M` 2 с → бейдж
  «ГОВОРИТЕ» → после отпускания текст вставляется (нужен движок и микрофон).

Логи: `E:\Program Files\Klavium\data\logs\app.log`. Стартовая строка:

```
хоткеи: исправление=…, отмена=…, выделенное=…, транслит=…, подсказка=…, голос=…, выход=…
```

## 6. Грабли (проверено на практике, не повторять)

1. **Вторая копия приложения.** `target\release\klavium.exe` — это отдельная копия со
   своим каталогом данных `target\release\data` и **дефолтными** хоткеями. Пользователь
   уже попадал в это: «отмена не работает», потому что была запущена копия из репозитория,
   а настройки жили в установленной. Всегда проверять `Path` процесса и каталог данных
   в журнале.
2. **Статус хука нельзя строить на `GetLastInputInfo`** — он обновляется и от мыши,
   поэтому клик по окну даёт ложное «хук не получает клавиатуру». Использовать свежесть
   последнего колбэка (`hook::last_event_age_ms`).
3. **Поток трея не должен держать `Arc` движка.** Из-за этого `events_thread.join()` на
   выходе никогда не завершался, приложение висело, дымовые прогоны уходили в TIMEOUT.
   Для паузы сделана слабая ссылка (`EnginePause(Weak<CorrectionEngine>)`).
4. **Синтетический `SendInput` без `wScan`** не матчится для `Trigger::Char` — хоткей
   на букве «не срабатывает» в тесте, хотя код верный. Передавать скан-код
   (`M` = `0x32`).
5. **`release_modifiers()` перед любой инъекцией** — иначе Backspace уходит как
   `Ctrl+Shift+Backspace` и текст дописывается вместо замены.
6. **`Ctrl+C`/`Ctrl+V` одной пачкой `SendInput`** — раздельные вызовы теряют модификатор
   (в поле уходит «v»).
7. **`GetClipboardSequenceNumber` вместо сравнения содержимого** — иначе повторное
   копирование того же текста считается «пустым выделением».
8. **Автопереустановка хука «на месте»** (`Unhook` + `SetWindowsHookEx` в том же потоке)
   убивает хук. Оставлен только WARN в журнале; для самовосстановления нужно пересоздавать
   поток хука.
9. **`Pause`/`PrintScreen`/`ScrollLock`** нельзя назначить нажатием: при своём фокусе окно
   не получает клавишу, хук её не видит. Только ручной ввод; raw input отложен.
10. **Clippy-лимиты**: функции длиннее 100 строк и `struct_excessive_bools` включены
    в workspace-линтах — длинные методы разбивать на смысловые части (`draw_voice`
    уже разбивался, `diagnostics_state` тоже).
11. PowerShell: строка `"$p:$x"` ломается на `$p:` — писать `"${p}:$x"`.
    Here-string с двойными кавычками интерполирует `$` — для текста с `$` использовать `@'…'@`.
12. **Синтетический ввод в чужие окна ненадёжен.** Новый Блокнот Windows 11 не принимает
    ни `SendKeys`, ни `SendInput` (окно находится, текст не появляется). `SendKeys`
    вдобавок не доходит до хука (в журнале нет строк `слово:`), а события со скан-кодом —
    доходят. Поэтому живые проверки — на своём поле WinForms и только скан-кодами,
    как в `tools\check-translit.ps1`; для самого поля — `Application.DoEvents` в цикле.

## 7. Файловая карта

| Путь | Что там |
|---|---|
| `crates/klavium-core/src/` | Логика без Win32: `config.rs` (вся схема настроек), `detect.rs` (детекция), `convert.rs` (раскладки), `dicts.rs` (FST-словари), `apprules.rs` (правила приложений), `hotkeys.rs` (разбор и матчинг), `stats.rs`, `voice.rs` |
| `crates/klavium-core/build.rs` | Сборка FST-словарей из текстовых списков |
| `crates/klavium-platform/src/` | Win32: `hook.rs` (хук, сторож, `last_event_age_ms`, `slow_callback_count`), `inject.rs` (ввод, буфер, выделение), `window.rs` (окна, каретка, `foreground_process_name`), `overlay.rs` (карточки/тосты/бейджи), `layout.rs`, `mem.rs`, `autostart.rs`, `single_instance.rs` |
| `crates/klavium-ui/src/` | `settings.rs` (все секции; `Section`, `HotkeyField`, `SettingsContext`), `theme.rs` (токены), `widgets.rs` (компоненты), `tray.rs` (иконка, меню, `PauseAccess`) |
| `crates/klavium-app/src/` | `main.rs` (сборка приложения, хук, трей, окно), `engine.rs` (детекция/очередь/коррекция/отмена/пауза), `real_io.rs` (Win32-ввод, скорость), `runtime.rs` (`engine_config`, событийный цикл), `hotkeys.rs` (`CompiledHotkeys`), `voice/` (`mod.rs`, `worker.rs`, `push.rs`, `audio.rs`, `engines/`), `services.rs`, `stats.rs`, `sound.rs`, `models.rs`, `config_io.rs` |
| `tools/` | `build_dist.ps1` (NSIS-установщик + portable zip), `gen_icons.py`, скрипты проверки (см. таблицу выше) |
| `docs/` | `TZ-0.2.md` (ТЗ и статусы), `architecture.md`, `images/` (скриншоты для README) |

## 8. Чек-лист перед коммитом

- [ ] `cargo fmt --all --check` — чисто
- [ ] `cargo clippy --workspace --all-targets` — без предупреждений
- [ ] `cargo test --workspace` — все зелёные
- [ ] версия в `Cargo.toml` поднята
- [ ] проверка функции вживую по журналу (не только тесты)
- [ ] все 9 дымовых прогонов — код 0
- [ ] `tools\build_dist.ps1 -SkipBuild` — установщик собрался
- [ ] скриншоты в `docs/images` обновлены, если менялся интерфейс
- [ ] `docs/TZ-0.2.md` — пункт помечен ✅ с версией
- [ ] коммит по-русски, `git push origin main`
