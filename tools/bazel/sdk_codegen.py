"""Run all SDK generators in an isolated, declared output tree."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

import grpc_tools.protoc

parser = argparse.ArgumentParser()
for name in ['output', 'config', 'protoc', 'go', 'go-grpc', 'java', 'es', 'connect', 'rust', 'clientsets', 'gofmt']:
    parser.add_argument('--' + name, required=True)
args = parser.parse_args()
config = json.loads(Path(args.config).read_text())
root = Path(args.output).absolute()
root.mkdir(parents=True, exist_ok=True)
for directory in ['proto', 'third_party']:
    shutil.copytree(directory, root / directory, dirs_exist_ok=True)
tools = {name: str(Path(getattr(args, name.replace('-', '_'))).absolute()) for name in ['protoc','go','go-grpc','java','es','connect','rust','clientsets','gofmt']}
env = dict(os.environ)
if env.get("RUNFILES_DIR"):
    env["RUNFILES"] = env["RUNFILES_DIR"]
env.update(CODEGEN_ROOT=str(root), PROTOC=tools['protoc'], PYTHONPATH=os.pathsep.join(sys.path))

def run(command):
    child_env = dict(env)
    if command[0] != sys.executable:
        child_env.pop("PYTHONPATH", None)
    subprocess.run(command, cwd=root, env=child_env, check=True)

protos = config['protos']
common = ['-I.', '-Ithird_party/googleapis', '--experimental_allow_proto3_optional']
go_out = 'sdk/go/talon-client'
java_out = 'sdk/java/talon-client/src/main/java'
js_out = 'sdk/js/talon-client/src/gen'
py_out = 'sdk/python/talon-client/src/talon_client'
for directory in [go_out,java_out,js_out,py_out]:
    (root / directory).mkdir(parents=True, exist_ok=True)
run([tools['protoc'], *common, '--plugin=protoc-gen-go=' + tools['go'], '--plugin=protoc-gen-go-grpc=' + tools['go-grpc'], '--go_out=' + go_out, '--go-grpc_out=' + go_out, *config['go_options'], *protos])
run([tools['protoc'], *common, '--plugin=protoc-gen-grpc-java=' + tools['java'], '--java_out=' + java_out, '--grpc-java_out=' + java_out, *protos])
run([tools['protoc'], *common, '--plugin=protoc-gen-es=' + tools['es'], '--plugin=protoc-gen-connect-es=' + tools['connect'], '--es_out=' + js_out, '--es_opt=target=ts,import_extension=.js', '--connect-es_out=' + js_out, '--connect-es_opt=target=ts,import_extension=.js', *protos])
run([sys.executable, '-m', 'grpc_tools.protoc', *common, '--python_out=' + py_out, '--pyi_out=' + py_out, '--grpc_python_out=' + py_out, *protos])
for path in (root / java_out).rglob('*.java'):
    path.write_text('\n'.join(line.rstrip() for line in path.read_text().splitlines()) + '\n')
for path in (root / js_out).rglob('*.ts'):
    path.write_text('\n'.join(line.rstrip() for line in path.read_text().splitlines()).rstrip() + '\n')
for directory in [root / py_out, *(root / py_out).rglob('*')]:
    if directory.is_dir():
        (directory / '__init__.py').touch()
for path in (root / py_out).rglob('*'):
    if path.name.endswith(('_pb2.py', '_pb2.pyi', '_pb2_grpc.py')):
        text = path.read_text()
        for package in ['', '.data', '.external', '.harness', '.resources', '.talon', '.talon.v1']:
            text = text.replace(f'from proto{package} import ', f'from talon_client.proto{package} import ')
        text = text.replace('import proto.', 'import talon_client.proto.')
        path.write_text(text)
run([tools['clientsets']])
run([tools['gofmt'], '-w', go_out + '/clientset_gen.go'])
run([tools['rust'], str(root)])
# These were inputs needed by the generators, not generated SDK artifacts.
shutil.rmtree(root / 'proto')
shutil.rmtree(root / 'third_party')
