#!/usr/bin/env python3
"""Checagens estáticas da prova isolada; não executa o aplicativo."""
import hashlib
import pathlib
import re
import subprocess
import sys
import tempfile
import zipfile


def check_jni(kotlin, symbols):
    text = pathlib.Path(kotlin).read_text(encoding='utf-8')
    methods = set(re.findall(r'@JvmStatic\s+external\s+fun\s+(\w+)', text))
    if 'start' not in methods or 'frame' not in methods:
        raise ValueError('Declarações JNI esperadas não encontradas')
    exported = {line.split()[-1] for line in pathlib.Path(symbols).read_text().splitlines() if line.split()}
    missing = {'Java_org_holool_gpui_GpuiNative_' + name for name in methods} - exported
    if missing:
        raise ValueError('Símbolos JNI ausentes: ' + ', '.join(sorted(missing)))
    print(f'{len(methods)} símbolos JNI verificados')


def check_apk(apk, library, readelf, checker):
    expected = 'lib/arm64-v8a/libhorizon_gpui_prova.so'
    expected_sha = hashlib.sha256(pathlib.Path(library).read_bytes()).digest()
    with zipfile.ZipFile(apk) as archive, tempfile.TemporaryDirectory() as tmp:
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError('O APK contém entradas duplicadas')
        libs = [n for n in names if n.startswith('lib/') and n.endswith('.so')]
        if expected not in libs:
            raise ValueError('Biblioteca da prova ausente no APK')
        if any(not n.startswith('lib/arm64-v8a/') for n in libs):
            raise ValueError('ABI inesperada no APK de teste')
        if hashlib.sha256(archive.read(expected)).digest() != expected_sha:
            raise ValueError('Biblioteca no APK difere da biblioteca compilada')
        for index, name in enumerate(libs):
            path = pathlib.Path(tmp) / f'{index}.so'
            path.write_bytes(archive.read(name))
            subprocess.run([sys.executable, checker, readelf, str(path)], check=True)
        print(f'{len(libs)} biblioteca(s) verificadas no APK arm64')


if __name__ == '__main__':
    if len(sys.argv) == 4 and sys.argv[1] == 'jni':
        check_jni(*sys.argv[2:])
    elif len(sys.argv) == 6 and sys.argv[1] == 'apk':
        check_apk(*sys.argv[2:])
    else:
        raise SystemExit('Uso: check-apk-ci.py jni KOTLIN SIMBOLOS | apk APK LIB READELF CHECKER')
