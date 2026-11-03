#!/usr/bin/env python3
"""Fixture interativa de rastreamento de mouse SGR (?1006h + ?1003h) para o MaxTerminal."""

import sys
import termios
import tty


def main() -> None:
    if not sys.stdin.isatty():
        sys.stdout.write("Modo não-interativo: stdin não é um TTY.\n")
        return

    fd = sys.stdin.fileno()
    old_settings = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)
        sys.stdout.write("\x1b[?1006h\x1b[?1003h")
        sys.stdout.write("Mouse SGR ativo (?1006h + ?1003h). Pressione 'q' ou Ctrl+C para sair.\r\n")
        sys.stdout.flush()

        while True:
            ch = sys.stdin.read(1)
            if not ch or ch in ("q", "Q", "\x03"):
                break
            sys.stdout.write(f"Byte recebido: {repr(ch)}\r\n")
            sys.stdout.flush()
    finally:
        sys.stdout.write("\x1b[?1003l\x1b[?1006l\r\nMouse SGR desativado.\r\n")
        sys.stdout.flush()
        termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)


if __name__ == "__main__":
    main()
