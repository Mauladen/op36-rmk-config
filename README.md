# Beekeeb 3W6HS (RMK)

Прошивка клавиатуры **Beekeeb 3W6HS** (36 клавиш, split 3×5+3) на базе
[RMK](https://rmk.rs) под **RP2040** по проводу: USB в левой половине, правая
сидит на кабеле через I2C-экспандер **TCA9555**. Раскладка — 6 слоёв с алиасами
Universal Layout (LatchTap/WM/OSM — их держит `patch/wellum.patch`) и живёт
целиком в `keyboard.toml`.

```
keyboard.toml       раскладка/матрица/поведения — единственный источник правды
src/main.rs         entry: запуск матриц, клавиатуры, USB
src/right_matrix.rs скан правой половины: TCA9555 по I2C0 (события row 4–7, col 0–4)
patch/wellum.patch  отличие от upstream RMK: язык алиасов раскладки
patch/rmk-revision  пин ревизии RMK
Makefile.toml       cargo make build / cargo make uf2
```

## Железо

- **RP2040** — левая половина, туда же приходит USB.
- Матрица левой половины: rows = GP7–GP10, cols = GP11–GP15.
- Правая половина — TCA9555 (7-bit адрес 0x20) на I2C0: SDA = GP0, SCL = GP1.
  Строки 4–7 — GPIOB0–B3 (активны низким уровнем), колонки читаются из
  GPIOA (реверс битов, как в QMK).

## Сборка

> Пререквизиты: `cargo-make`; соседний checkout официального **`rmk-rs/rmk`**
> на ревизии из `patch/rmk-revision` с применённым `patch/wellum.patch` — это
> `path`-зависимость `../rmk/rmk` в `Cargo.toml`. Пути внутри патча заданы
> относительно корня rmk, поэтому применяется он оттуда:
> `git -C ../rmk apply --check ../3w6hs-rmk-config/patch/wellum.patch && git -C ../rmk apply ../3w6hs-rmk-config/patch/wellum.patch`.
> Тулчейн и target `thumbv6m-none-eabi` rustup ставит сам из
> `rust-toolchain.toml` при первом запуске cargo.

```shell
cargo make build   # cargo build --release
cargo make uf2     # + objcopy ihex → hex-to-uf2 (family rp2040)
```

Результат — один файл `firmware/3w6hs.uf2`.

GitHub Actions собирает тот же файл и публикует его в artifact
`firmware-<commit SHA>` соответствующего workflow run.

## Прошивание

Зажать на левой половине кнопку **BOOTSEL** и вставить USB — плата появится
как накопитель `RPI-RP2`; перетащить на него `firmware/3w6hs.uf2`.

## Лицензия

MIT OR Apache-2.0 (как и RMK).
