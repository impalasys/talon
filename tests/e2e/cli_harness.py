import json
import os
import subprocess


class TalonCli:
    def __init__(
        self,
        binary,
        gateway,
        *,
        grpc_web=False,
        token=None,
        api_key=None,
        jwt_secret=None,
        password=None,
        env=None,
        timeout=120,
    ):
        self.binary = str(binary)
        self.gateway = gateway
        self.grpc_web = grpc_web
        self.token = token
        self.api_key = api_key
        self.jwt_secret = jwt_secret
        self.password = password
        self.env = env
        self.timeout = timeout

    def base_args(self):
        args = [self.binary, "--gateway", self.gateway]
        if self.grpc_web:
            args.append("--grpc-web")
        if self.token:
            args.extend(["--token", self.token])
        if self.api_key:
            args.extend(["--api-key", self.api_key])
        if self.jwt_secret:
            args.extend(["--jwt-secret", self.jwt_secret])
        if self.password:
            args.extend(["--password", self.password])
        return args

    def run(self, *args, timeout=None, check=True):
        env = os.environ.copy()
        if self.env:
            env.update(self.env)
        result = subprocess.run(
            [*self.base_args(), *map(str, args)],
            text=True,
            capture_output=True,
            check=False,
            timeout=timeout or self.timeout,
            env=env,
        )
        if check and result.returncode != 0:
            command = " ".join([*self.redacted_base_args(), *map(str, args)])
            raise AssertionError(
                f"talon-cli failed: {command}\n"
                f"exit={result.returncode}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            )
        return result

    def json(self, *args, timeout=None):
        result = self.run(*args, timeout=timeout)
        try:
            return json.loads(result.stdout)
        except json.JSONDecodeError as err:
            command = " ".join([*self.redacted_base_args(), *map(str, args)])
            raise AssertionError(
                f"talon-cli did not emit JSON: {command}\n"
                f"error={err}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            ) from err

    def redacted_base_args(self):
        redacted = []
        args = self.base_args()
        redact_next = False
        for arg in args:
            if redact_next:
                redacted.append("<redacted>")
                redact_next = False
                continue
            redacted.append(arg)
            if arg in {"--token", "--api-key", "--jwt-secret", "--password"}:
                redact_next = True
        return redacted
