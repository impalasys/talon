# Talon Cloudflare Terraform Module

Reusable Terraform module for Talon's Cloudflare backing resources.

This module intentionally manages Cloudflare infrastructure resources, not the Worker bundle itself. Wrangler remains responsible for deploying `infra/cf/worker`, and CI should provide pinned Cloudflare Container image tags or digests for production deploys.

## Resources

- D1 database for Talon control-plane state.
- R2 bucket for Talon object storage.
- Three Cloudflare Queues for session dispatch, resource lifecycle, and session control.

## Usage

```hcl
module "talon_cf" {
  source = "github.com/impalasys/talon//infra/cf/tf?ref=main"

  account_id  = var.cloudflare_account_id
  name_prefix = "talon"
}
```

Pin `ref` to a release tag or commit SHA for production deploys, for example `?ref=v0.1.0` or `?ref=<commit-sha>`.

For a future standalone registry module, this directory can be copied into a `terraform-cloudflare-talon` repository and tagged there.
