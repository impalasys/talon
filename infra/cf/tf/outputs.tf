output "account_id" {
  description = "Cloudflare account ID used by this module."
  value       = var.account_id
}

output "d1_database" {
  description = "D1 database binding metadata for Wrangler or generated Worker config."
  value = {
    binding       = "TALON_D1"
    database_name = cloudflare_d1_database.control_plane.name
    database_id   = cloudflare_d1_database.control_plane.id
  }
}

output "r2_bucket" {
  description = "R2 bucket binding metadata for Wrangler or generated Worker config."
  value = {
    binding     = "TALON_R2"
    bucket_name = cloudflare_r2_bucket.objects.name
  }
}

output "queues" {
  description = "Queue metadata keyed by Talon queue role."
  value = {
    for role, queue in cloudflare_queue.talon : role => {
      binding    = local.queue_bindings[role].binding
      topic      = local.queue_bindings[role].topic
      queue_name = queue.queue_name
      queue_id   = queue.id
    }
  }
}

output "wrangler_bindings" {
  description = "Resource names in a shape that can be consumed by a future Wrangler config generator."
  value = {
    d1_databases = [
      {
        binding       = "TALON_D1"
        database_name = cloudflare_d1_database.control_plane.name
        database_id   = cloudflare_d1_database.control_plane.id
      }
    ]
    r2_buckets = [
      {
        binding     = "TALON_R2"
        bucket_name = cloudflare_r2_bucket.objects.name
      }
    ]
    queues = {
      producers = [
        for role, queue in cloudflare_queue.talon : {
          binding = local.queue_bindings[role].binding
          queue   = queue.queue_name
        }
      ]
      consumers = [
        for _, queue in cloudflare_queue.talon : {
          queue          = queue.queue_name
          max_batch_size = 10
        }
      ]
    }
  }
}

output "tags" {
  description = "Caller-provided tags passed through for downstream config generation."
  value       = var.tags
}
