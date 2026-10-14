import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
spec = importlib.util.spec_from_file_location('release_plan', Path(__file__).with_name('release_plan.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ReleaseSelection(unittest.TestCase):
    def test_manifest_bump_requires_no_release_script_change(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            contract = {'packages': [{'name': '@owner/tool', 'manifest': 'package.json', 'ecosystem': 'npm'}]}
            for version in ['0.2.0', '0.3.0', '1.0.0-beta.1']:
                (root / 'package.json').write_text(json.dumps({'name': '@owner/tool', 'version': version}))
                self.assertEqual(module.select_package(root, contract, 'npm', 'v' + version)['version'], version)
                with self.assertRaises(ValueError): module.select_package(root, contract, 'npm', 'v9.9.9')
    def test_monorepo_selects_only_named_package(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); packages = []
            for name in ['a', 'b']:
                (root / name).mkdir(); (root / name / 'package.json').write_text(json.dumps({'name': '@owner/' + name, 'version': '0.2.0'}))
                packages.append({'name': '@owner/' + name, 'manifest': name + '/package.json', 'ecosystem': 'npm'})
            contract = {'packages': packages}
            self.assertEqual(module.select_package(root, contract, 'npm', 'npm/owner--a/v0.2.0')['name'], '@owner/a')
            with self.assertRaises(ValueError): module.select_package(root, contract, 'npm', 'v0.2.0')
    def test_private_identity_and_escaping_manifest_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'package.json').write_text(json.dumps({'name': 'other', 'version': '0.2.0', 'private': True}))
            for manifest in ['package.json', '../package.json', '/tmp/package.json']:
                with self.assertRaises(ValueError): module.select_package(root, {'packages': [{'name': 'accepted', 'manifest': manifest, 'ecosystem': 'npm'}]}, 'npm', 'v0.2.0')
    def test_workspace_crate_version_and_publish_false_boundary(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory); (root / 'member').mkdir()
            (root / 'Cargo.toml').write_text('[workspace.package]\nversion="0.2.0"\n')
            path = root / 'member/Cargo.toml'
            path.write_text('[package]\nname="tool"\nversion.workspace=true\npublish=["crates-io"]\n')
            contract = {'packages': [{'name': 'tool', 'manifest': 'member/Cargo.toml', 'ecosystem': 'cargo'}]}
            self.assertEqual(module.select_package(root, contract, 'cargo', 'cargo/tool/v0.2.0')['version'], '0.2.0')
            path.write_text('[package]\nname="tool"\nversion="0.2.0"\npublish=false\n')
            with self.assertRaises(ValueError): module.select_package(root, contract, 'cargo', 'v0.2.0')
    def test_source_rejects_wrong_owner_and_branch_before_network(self):
        contract = {'repository': 'owner/repo'}
        for environment in [{'GITHUB_REPOSITORY': 'wrong/repo'}, {'GITHUB_REPOSITORY': 'owner/repo', 'GITHUB_REF_TYPE': 'branch'}]:
            with self.assertRaises(ValueError): module.verify_source(Path('.'), contract, environment)

if __name__ == '__main__': unittest.main()
