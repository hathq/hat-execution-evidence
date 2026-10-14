import importlib.util
from io import BytesIO
from pathlib import Path
import json, sys, tarfile, tempfile, unittest
sys.path.insert(0, str(Path(__file__).parent))
import archive_binding as module
class ArchiveBinding(unittest.TestCase):
    def archive(self, path, manifest, extras=()):
        with tarfile.open(path, 'w:gz') as tar:
            data=json.dumps(manifest).encode();entry=tarfile.TarInfo('package/package.json');entry.size=len(data);tar.addfile(entry,BytesIO(data))
            for entry in extras:tar.addfile(entry,BytesIO(b''))
    def test_wrong_version_and_private_archive_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'fixture.tgz'
            for manifest in [{'name':'fixture','version':'0.1.0'}, {'name':'fixture','version':'0.2.0','private':True}]:
                self.archive(path,manifest)
                with self.assertRaises(ValueError):module.inspect_archive(path,'npm','fixture','0.2.0')
            self.archive(path,{'name':'fixture','version':'0.2.0'})
            module.inspect_archive(path,'npm','fixture','0.2.0')
    def test_path_escape_symlink_and_runtime_environment_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'fixture.tgz'
            for name, kind in [('../escape',tarfile.REGTYPE),('package/link',tarfile.SYMTYPE),('package/.env',tarfile.REGTYPE)]:
                entry=tarfile.TarInfo(name);entry.type=kind;entry.linkname='/tmp/other'
                self.archive(path,{'name':'fixture','version':'0.2.0'},[entry])
                with self.assertRaises(ValueError):module.inspect_archive(path,'npm','fixture','0.2.0')
if __name__=='__main__':unittest.main()
