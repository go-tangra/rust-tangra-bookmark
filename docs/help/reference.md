---
title: Reference
summary: Bookmark and share fields, permissions, roles, API routes and limits.
kind: reference
order: 60
tags: [bookmarks, reference, permissions, api, limits]
---
## Bookmark fields

| Field | Meaning | Limit |
|---|---|---|
| `id` | bookmark ID (UUID) | |
| `url` | `http` or `https` address | 2048 characters |
| `title` | title | 500 characters |
| `description` | description (several lines) | 5000 characters |
| `tags` | tags | 20 tags of 1–64 characters |
| `created_by` | user ID of the creator | |
| `created_at`, `updated_at` | times (RFC 3339) | |
| `permissions` | what you may do: `read`, `write`, `delete`, `share` | |

## Shares

| Field | Meaning |
|---|---|
| `resource_id` | the bookmark; empty = every bookmark of the organisation |
| `subject_type` | `user`, `role` or `tenant` (everyone) |
| `subject_id` | user ID, role slug, empty for everyone |
| `relation` | `owner`, `editor`, `sharer`, `viewer` |
| `expires_at` | optional end of the access |

## Permissions

| Permission | Allows |
|---|---|
| `bookmarks:read` | list and open bookmarks you have access to |
| `bookmarks:create` | create bookmarks |
| `bookmarks:update` | change bookmarks you may edit |
| `bookmarks:delete` | remove bookmarks you own |
| `permissions:manage` | share bookmarks and take shares back |
| `backup:manage` | export and import the organisation's bookmarks |

Built-in roles: `owner` and `admin` hold every permission; `member` and
`operator` hold the bookmark permissions and sharing; `auditor` holds
`bookmarks:read`. Module roles: **Bookmarks administrator** (every
permission) and **Bookmarks viewer** (`bookmarks:read`). The permission
lets you use an operation; the share level decides on which bookmarks.

## API

Under `/api/bookmark/v1` through the portal gateway:

| Route | Permission |
|---|---|
| `GET /bookmarks`, `GET /bookmarks/{id}` | `bookmarks:read` |
| `POST /bookmarks` | `bookmarks:create` |
| `PUT /bookmarks/{id}` | `bookmarks:update` |
| `POST /bookmarks/{id}/remove` | `bookmarks:delete` |
| `GET /permissions`, `POST /permissions/grant`, `POST /permissions/revoke` | `permissions:manage` |
| `POST /permissions/check`, `GET /permissions/effective` | `bookmarks:read` |
| `POST /backup/export`, `POST /backup/import` | `backup:manage` |

Lists take `page`, `page_size` (at most 200), `sort` and `order`; the
bookmark list also `q` (at most 200 characters) and `tag` (up to 10).
Errors are `{"reason": ...}`: `not_found` also for bookmarks you may not
open, `forbidden` when you may open it but not do the action,
`validation_failed` with the offending fields, `conflict` for a share that
exists.
