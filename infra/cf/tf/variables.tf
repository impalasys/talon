variable "account_id" {
  description = "Cloudflare account ID where Talon resources will be created."
  type        = string
}

variable "name_prefix" {
  description = "Prefix used for Cloudflare resource names."
  type        = string
  default     = "talon"
}

variable "d1_database_name" {
  description = "Name of the D1 database used for Talon control-plane state."
  type        = string
  default     = null
}

variable "d1_jurisdiction" {
  description = "Optional D1 jurisdiction, such as eu. Leave null for Cloudflare default placement."
  type        = string
  default     = null
}

variable "d1_primary_location_hint" {
  description = "Optional D1 primary location hint, such as wnam or enam. Leave null for Cloudflare default placement."
  type        = string
  default     = null
}

variable "d1_read_replication_mode" {
  description = "D1 read replication mode. Defaults to disabled so Terraform does not try to clear Cloudflare's returned read_replication object."
  type        = string
  default     = "disabled"

  validation {
    condition     = contains(["auto", "disabled"], var.d1_read_replication_mode)
    error_message = "d1_read_replication_mode must be either auto or disabled."
  }
}

variable "r2_bucket_name" {
  description = "Name of the R2 bucket used for Talon object storage."
  type        = string
  default     = null
}

variable "queue_names" {
  description = "Cloudflare Queue names keyed by Talon queue role."
  type = object({
    session_dispatch   = optional(string)
    resource_lifecycle = optional(string)
    session_control    = optional(string)
    index_events       = optional(string)
  })
  default = {}
}

variable "queue_settings" {
  description = "Optional Cloudflare Queue settings applied to all Talon queues."
  type = object({
    delivery_delay           = optional(number)
    delivery_paused          = optional(bool)
    message_retention_period = optional(number)
  })
  default = null
}

variable "tags" {
  description = "Free-form labels exposed through module outputs for callers that mirror names into generated config or CI metadata."
  type        = map(string)
  default     = {}
}
