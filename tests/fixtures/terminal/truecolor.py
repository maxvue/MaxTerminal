#!/usr/bin/env python3
"""Fixture de gradiente TrueColor RGB de 24 bits (80 colunas x 3 faixas) para o MaxTerminal."""

import sys


def render_gradient_band(label: str, mode: int, cols: int = 80) -> None:
    sys.stdout.write(f"{label}\n")
    for row in range(2):
        line_parts = []
        for col in range(cols):
            t = int((col / max(1, cols - 1)) * 255)
            if mode == 0:
                r, g, b = 255 - t, t, 64
            elif mode == 1:
                r, g, b = 32, 255 - t, t
            else:
                r, g, b = t, 64, 255 - t
            fg_r, fg_g, fg_b = 255 - r, 255 - g, 255 - b
            char = "▀" if row % 2 == 0 else "▄"
            line_parts.append(f"\x1b[48;2;{r};{g};{b}m\x1b[38;2;{fg_r};{fg_g};{fg_b}m{char}")
        line_parts.append("\x1b[0m\n")
        sys.stdout.write("".join(line_parts))


def main() -> None:
    render_gradient_band("=== Faixa 1: Vermelho -> Verde (TrueColor 24-bit) ===", 0)
    render_gradient_band("=== Faixa 2: Verde -> Azul (TrueColor 24-bit) ===", 1)
    render_gradient_band("=== Faixa 3: Azul -> Vermelho (TrueColor 24-bit) ===", 2)
    sys.stdout.write("\x1b[0m=== TRUECOLOR FINALIZADO ===\n")
    sys.stdout.flush()


if __name__ == "__main__":
    main()
