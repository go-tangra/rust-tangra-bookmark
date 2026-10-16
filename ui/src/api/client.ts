// The bookmark API through the gateway: the kit client bound to this module's base.
import { createApi, ApiError, csrfToken, describe, type Method, type RequestOptions } from '@go-tangra/ui/api'
import type { paths } from './schema.d'

export { ApiError, csrfToken, describe }
export type { Method, RequestOptions }

// Path names are checked against the OpenAPI contract at compile time.
export type ApiPath = keyof paths
export const BASE = '/api/bookmark/v1'

export const api = createApi({ base: BASE })
