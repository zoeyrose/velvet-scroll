import importlib.util
import pathlib
import stat
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('extract', pathlib.Path(__file__).with_name('extract.py'))
extract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(extract)


class ExtractionTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = pathlib.Path(self.temporary.name)

    def archive(self, entries):
        path = self.root / 'artifact.zip'
        with zipfile.ZipFile(path, 'w') as archive:
            for name, data in entries:
                archive.writestr(name, data)
        return path

    def test_static_files_only_and_trusted_headers(self):
        path = self.archive([('index.html', '<h1>Preview</h1>'), ('assets/site.js', 'console.log(1)'), ('assets/site.css', 'body {}'), ('_headers', '/*\n Access-Control-Allow-Origin: *')])
        output = self.root / 'out'
        self.assertEqual(extract.extract_static(path, output), 3)
        self.assertEqual((output / '_headers').read_text(), extract.HEADERS)
        self.assertEqual((output / 'assets/site.js').read_text(), 'console.log(1)')

    def test_rejects_traversal_hidden_paths_and_control_characters(self):
        for name in ['../outside.html', '/outside.html', 'assets/../../outside.js', 'assets\\evil.js', '.env', '.git/config', 'x//index.html', 'C:/evil.html', 'bad\nname.html']:
            with self.subTest(name=name):
                path = self.archive([('index.html', 'ok'), (name, 'malicious')])
                with self.assertRaises(ValueError):
                    extract.extract_static(path, self.root / 'out')
                self.assertFalse((self.root / 'out').exists())

    def test_rejects_workers_functions_config_and_non_static_files(self):
        for name in ['_worker.js', '_worker.js/index.js', '_routes.json', '_redirects', 'functions/index.js', 'FUNCTIONS/index.js', 'wrangler.json', 'wrangler.jsonc', 'wrangler.toml', 'package.json', 'node_modules/a/index.js', 'evil.sh', 'deploy.py', 'site.wasm']:
            with self.subTest(name=name):
                path = self.archive([('index.html', 'ok'), (name, 'malicious')])
                with self.assertRaises(ValueError):
                    extract.extract_static(path, self.root / 'out')
                self.assertFalse((self.root / 'out').exists())

    def test_rejects_symlinks_special_files_and_case_collisions(self):
        for mode in [stat.S_IFLNK | 0o777, stat.S_IFIFO | 0o600]:
            entry = zipfile.ZipInfo('asset.js')
            entry.create_system = 3
            entry.external_attr = mode << 16
            path = self.archive([('index.html', 'ok'), (entry, '/etc/passwd')])
            with self.assertRaises(ValueError):
                extract.extract_static(path, self.root / 'out')
        path = self.archive([('index.html', 'ok'), ('INDEX.html', 'different')])
        with self.assertRaises(ValueError):
            extract.extract_static(path, self.root / 'out')

    def test_limits_archive_size_and_requires_index(self):
        path = self.archive([('asset.js', 'no index')])
        with self.assertRaises(ValueError):
            extract.extract_static(path, self.root / 'out')
        path = self.archive([('index.html', 'x' * (extract.MAX_FILE_SIZE + 1))])
        with self.assertRaises(ValueError):
            extract.extract_static(path, self.root / 'out')
        path = self.archive([(f'asset{i}.txt', '') for i in range(extract.MAX_FILES + 1)])
        with self.assertRaises(ValueError):
            extract.extract_static(path, self.root / 'out')

    def test_never_overwrites_an_existing_destination(self):
        path = self.archive([('index.html', 'ok')])
        (self.root / 'out').mkdir()
        with self.assertRaises(ValueError):
            extract.extract_static(path, self.root / 'out')


if __name__ == '__main__':
    unittest.main()
