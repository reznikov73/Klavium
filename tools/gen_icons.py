"""Генерация иконок Klavium: трей (EN/RU/нейтральная) и иконка приложения (.ico).

Запуск:   python tools/gen_icons.py
Требуется Pillow:   pip install pillow

Цвета из Python-эталона: синий акцент #2563eb, красный #dc2626;
нейтральная иконка — градиент иконки «RE» (#1e3c72 → #2a5298) с буквой «K».
"""

import pathlib
import tempfile

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent

BLUE = (37, 99, 235)
BLUE_DARK = (29, 78, 216)
RED = (220, 38, 38)
RED_DARK = (153, 27, 27)
NAVY = (30, 60, 114)
NAVY_LIGHT = (42, 82, 152)


def rounded_gradient(from_rgb, to_rgb, size):
    """Квадрат с вертикальным градиентом и закруглёнными углами (RGBA)."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    for y in range(size):
        t = y / max(size - 1, 1)
        color = tuple(int(from_rgb[i] + (to_rgb[i] - from_rgb[i]) * t) for i in range(3))
        draw.line([(0, y), (size, y)], fill=(*color, 255))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [0, 0, size - 1, size - 1], radius=size * 6 // 32, fill=255
    )
    img.putalpha(mask)
    return img


def with_text(base, text, size):
    """Белая жирная надпись с тенью по центру."""
    img = base.copy()
    draw = ImageDraw.Draw(img)
    font = None
    for name in ("arialbd.ttf", "segoeuib.ttf", "arial.ttf"):
        try:
            font = ImageFont.truetype(name, int(size * 0.55))
            break
        except OSError:
            continue
    if font is None:
        font = ImageFont.load_default()
    bbox = draw.textbbox((0, 0), text, font=font)
    x = (size - (bbox[2] - bbox[0])) / 2 - bbox[0]
    y = (size - (bbox[3] - bbox[1])) / 2 - bbox[1]
    shift = max(1, size // 64)
    draw.text((x + shift, y + shift), text, font=font, fill=(0, 0, 0, 90))
    draw.text((x, y), text, font=font, fill=(255, 255, 255, 255))
    return img


def main():
    size = 64
    en = with_text(rounded_gradient(BLUE, BLUE_DARK, size), "EN", size)
    ru = with_text(rounded_gradient(RED, RED_DARK, size), "RU", size)
    neutral = with_text(rounded_gradient(NAVY, NAVY_LIGHT, size), "K", size)

    ui_res = ROOT / "crates" / "klavium-ui" / "resources"
    ui_res.mkdir(parents=True, exist_ok=True)
    (ui_res / "tray_en.bin").write_bytes(en.tobytes())
    (ui_res / "tray_ru.bin").write_bytes(ru.tobytes())
    (ui_res / "tray_neutral.bin").write_bytes(neutral.tobytes())

    app_res = ROOT / "crates" / "klavium-app" / "resources"
    app_res.mkdir(parents=True, exist_ok=True)
    icon = with_text(rounded_gradient(NAVY, NAVY_LIGHT, 256), "K", 256)
    icon.save(
        app_res / "klavium.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )

    # Превью (крупно) — для визуальной проверки.
    preview = Image.new("RGBA", (size * 3 + 40, size + 20), (245, 245, 245, 255))
    for index, image in enumerate((en, ru, neutral)):
        preview.paste(image, (10 + index * (size + 10), 10), image)
    preview_path = pathlib.Path(tempfile.gettempdir()) / "klavium_icons_preview.png"
    preview.save(preview_path)
    print(f"OK: иконки записаны; превью: {preview_path}")


if __name__ == "__main__":
    main()
