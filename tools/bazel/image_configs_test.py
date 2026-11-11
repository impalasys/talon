"""Check that shipped configurations can resolve every inherited catalog."""
from pathlib import PurePosixPath
import sys
import tarfile
import unittest

import yaml


LAYER = sys.argv.pop()


class ImageConfigsTest(unittest.TestCase):
    def test_all_inherited_configs_are_in_the_image(self):
        with tarfile.open(LAYER) as layer:
            configs = {
                member.name: yaml.safe_load(layer.extractfile(member))
                for member in layer.getmembers()
                if member.isfile() and member.name.endswith(".yaml")
            }
        self.assertIn("data/talon/talon.yaml", configs)
        self.assertIn("data/talon/talon.docker-compose.yaml", configs)
        for name, config in configs.items():
            extends = config.get("extends", [])
            if isinstance(extends, str):
                extends = [extends]
            for relative in extends:
                inherited = str(PurePosixPath(name).parent / relative)
                self.assertIn(inherited, configs, f"{name} cannot load {relative}")


if __name__ == "__main__":
    unittest.main()
