import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('elf16k', Path(__file__).parents[1] / 'check-elf-16k.py')
elf = importlib.util.module_from_spec(spec)
spec.loader.exec_module(elf)

LOAD = '  LOAD 0x000000 0x0000000000000000 0x0000000000000000 0x001234 0x001234 R E 0x4000'
RELRO = '  GNU_RELRO 0x004000 0x0000000000004000 0x0000000000004000 0x004000 0x004000 R 0x1'

class AlignmentTests(unittest.TestCase):
    def test_valid(self):
        elf.check(LOAD + '\n' + RELRO)
    def test_small_alignment(self):
        with self.assertRaises(ValueError): elf.check(LOAD.replace('0x4000', '0x1000') + '\n' + RELRO)
    def test_load_offset_incongruent(self):
        with self.assertRaises(ValueError): elf.check(LOAD.replace('0x000000 ', '0x000001 ', 1) + '\n' + RELRO)
    def test_non_power_of_two_alignment(self):
        with self.assertRaises(ValueError): elf.check(LOAD.replace('0x4000', '0xc000') + '\n' + RELRO)
    def test_larger_alignment_needs_full_congruence(self):
        bad = LOAD.replace('0x000000 ', '0x004000 ', 1).replace('0x4000', '0x8000')
        with self.assertRaises(ValueError): elf.check(bad + '\n' + RELRO)
    def test_valid_larger_alignment(self):
        elf.check(LOAD.replace('0x4000', '0x8000') + '\n' + RELRO)
    def test_relro_end_unaligned(self):
        with self.assertRaises(ValueError): elf.check(LOAD + '\n' + RELRO.replace('0x004000 R', '0x004001 R'))
    def test_missing_segments(self):
        for data in ('', LOAD, RELRO):
            with self.subTest(data=data), self.assertRaises(ValueError): elf.check(data)
    def test_malformed(self):
        with self.assertRaises(ValueError): elf.check('LOAD 0x1\n' + RELRO)

if __name__ == '__main__': unittest.main()
