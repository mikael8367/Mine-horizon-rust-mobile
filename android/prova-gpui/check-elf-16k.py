#!/usr/bin/env python3
"""Verifica alinhamento ELF, independentemente de qualquer APK."""
import subprocess
import sys


def check(headers: str) -> None:
    loads = 0
    relro = False
    for line in headers.splitlines():
        fields = line.split()
        if fields and fields[0] == 'LOAD':
            if len(fields) < 8:
                raise ValueError(f'LOAD malformado: {line}')
            loads += 1
            alignment = int(fields[-1], 16)
            if (alignment < 16384 or alignment & (alignment - 1)
                    or (int(fields[1], 16) - int(fields[2], 16)) % alignment):
                raise ValueError(f'LOAD incompatível com páginas de 16 KiB: {line}')
        elif fields and fields[0] == 'GNU_RELRO':
            if len(fields) < 7:
                raise ValueError(f'GNU_RELRO malformado: {line}')
            relro = True
            if (int(fields[2], 16) + int(fields[5], 16)) % 16384:
                raise ValueError(f'GNU_RELRO incompatível com páginas de 16 KiB: {line}')
    if not loads or not relro:
        raise ValueError('ELF sem segmentos LOAD ou GNU_RELRO')


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit('Uso: check-elf-16k.py <llvm-readelf> <biblioteca.so>')
    headers = subprocess.check_output([sys.argv[1], '-lW', sys.argv[2]], text=True)
    check(headers)
    print('ELF: alinhamento LOAD/RELRO de 16 KiB validado (sem testar APK)')


if __name__ == '__main__':
    main()
