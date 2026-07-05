import uuid

from e2e.blackbox import assert_worker_registered
from e2e.stack import E2EStack
from talon_client import TalonClient


def test_worker_registers_local_endpoint(
    stack: E2EStack,
    client: TalonClient,
) -> None:
    # Verify that each stack's worker comes online and publishes a ready endpoint
    # that the control plane can discover.
    assert stack.worker_endpoint_url is not None
    assert_worker_registered(client, stack.worker_endpoint_url)
