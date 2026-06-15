locals {
  d1_database_name = coalesce(var.d1_database_name, "${var.name_prefix}-control-plane")
  r2_bucket_name   = coalesce(var.r2_bucket_name, "${var.name_prefix}-objects")

  queue_names = {
    session_dispatch   = coalesce(var.queue_names.session_dispatch, "${var.name_prefix}-session-dispatch")
    resource_lifecycle = coalesce(var.queue_names.resource_lifecycle, "${var.name_prefix}-resource-lifecycle")
    session_control    = coalesce(var.queue_names.session_control, "${var.name_prefix}-session-control")
  }

  queue_bindings = {
    session_dispatch = {
      binding = "SESSION_DISPATCH_QUEUE"
      topic   = "talon.session.dispatch"
    }
    resource_lifecycle = {
      binding = "RESOURCE_LIFECYCLE_QUEUE"
      topic   = "talon.resource.lifecycle"
    }
    session_control = {
      binding = "SESSION_CONTROL_QUEUE"
      topic   = "talon.session.control"
    }
  }
}

resource "cloudflare_d1_database" "control_plane" {
  account_id            = var.account_id
  name                  = local.d1_database_name
  jurisdiction          = var.d1_jurisdiction
  primary_location_hint = var.d1_primary_location_hint
}

resource "cloudflare_r2_bucket" "objects" {
  account_id = var.account_id
  name       = local.r2_bucket_name
}

resource "cloudflare_queue" "talon" {
  for_each = local.queue_names

  account_id = var.account_id
  queue_name = each.value
  settings   = var.queue_settings
}
