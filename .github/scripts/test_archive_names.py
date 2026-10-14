from pathlib import Path
import json, tempfile, unittest
from archive_names import archive_names
class ArchiveNames(unittest.TestCase):
    def test_scoped_package_version_bump_changes_filename_without_workflow_edit(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'.github').mkdir()
            (root/'.github/registry-release.json').write_text(json.dumps({'packages':[{'name':'@owner/tool','manifest':'package.json','ecosystem':'npm'}]}))
            for version in ['0.2.0','0.3.0']:
                (root/'package.json').write_text(json.dumps({'name':'@owner/tool','version':version}))
                self.assertEqual(archive_names(root),{'PACKAGE_ARCHIVE_OWNER_TOOL_NPM':'owner-tool-'+version+'.tgz'})
    def test_workspace_version_resolves_from_owner_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);(root/'.github').mkdir();(root/'member').mkdir()
            (root/'.github/registry-release.json').write_text(json.dumps({'packages':[{'name':'example','manifest':'member/Cargo.toml','ecosystem':'cargo'}]}))
            (root/'Cargo.toml').write_text('[workspace.package]\nversion="0.2.0"\n')
            (root/'member/Cargo.toml').write_text('[package]\nname="example"\nversion.workspace=true\n')
            self.assertEqual(archive_names(root),{'PACKAGE_ARCHIVE_EXAMPLE_CARGO':'example-0.2.0.crate'})
if __name__=='__main__':unittest.main()
