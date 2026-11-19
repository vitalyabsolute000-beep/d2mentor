# GSI notes

## Наблюдения на этой машине (2026-11-19)

- Dota 2 установлена в `C:\Program Files (x86)\Steam\steamapps\common\dota 2 beta`, вторая библиотека — `D:\SteamLibrary` (самой игры там нет).
- PTS Booster 0.2.47 прописала свой конфиг `gamestate_integration_kolokol.cfg`:
  - URI `http://127.0.0.1:38396/gsi`, throttle `0.5`, heartbeat `30.0`;
  - запрашивает `provider/map/player/hero/abilities/items/buildings/events/draft/minimap/roshan/couriers/neutralitems`, `wearables=0`.
- Лог PTS `gsi-agent\agent-output.log` показал: драфт врагов она получает **скриншотом** (`Windows Graphics Capture`, кадр `2560x116`), а не из GSI. Это подтверждает: в обычном матче GSI пики врагов не отдаёт.
- Кэш PTS: `%LOCALAPPDATA%\PTS Booster\gsi-agent` — не трогаем, чужие токены не читаем.

## Наш конфиг

`gamestate_integration_d2mentor.cfg`, порт **38397**, отдельный токен, файл рядом с PTS — конфликта нет (разные порты).

## Что проверить на живой игре

1. Приходит ли `draft` в обычном матче (ожидание: нет) и в Captain's Mode.
2. Есть ли `hero.xpos/ypos` вне режима наблюдателя.
3. Как меняются `items` при продаже/разборе/передаче (нужно для слот-вотчера).
4. Частота пакетов при `throttle 0.5` и задержка появления покупки в оверлее.
5. `clock_time` до 0:00 (пик/лоадинг) — отрицательный, обрабатываем.
6. Что приходит после конца матча (`POST_GAME`) и как быстро пропадает `map`.

## Как проверить без игры

Кнопка «Тест без Dota» в окне настроек отправляет фейковый пакет через тот же код, что и GSI.

Вручную:

```powershell
$token = Get-Content "$env:LOCALAPPDATA\D2Mentor\gsi-token.txt" -Raw
# подставить токен в JSON и отправить POST на http://127.0.0.1:38397/gsi
```
