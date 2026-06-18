import json
import os
import subprocess


class TalonCli:
    def __init__(
        self,
        binary,
        gateway,
        *,
        rest=False,
        token=None,
        jwt_secret=None,
        password=None,
        env=None,
        timeout=120,
    ):
        self.binary = str(binary)
        self.gateway = gateway
        self.rest = rest
        self.token = token
        self.jwt_secret = jwt_secret
        self.password = password
        self.env = env
        self.timeout = timeout

    def base_args(self):
        args = [self.binary, "--gateway", self.gateway]
        if self.rest:
            args.append("--rest")
        if self.token:
            args.extend(["--token", self.token])
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
            command = " ".join([*self.base_args(), *map(str, args)])
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
            command = " ".join([*self.base_args(), *map(str, args)])
            raise AssertionError(
                f"talon-cli did not emit JSON: {command}\n"
                f"error={err}\n"
                f"stdout:\n{result.stdout}\n"
                f"stderr:\n{result.stderr}"
            ) from err

