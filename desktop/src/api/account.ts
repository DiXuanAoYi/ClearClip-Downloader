import { invoke } from '@tauri-apps/api/core'
import type {
  AccountStatus,
  AccountSummary,
  RouteSpeed,
  RouteTest,
} from '../types'

export const accountApi = {
  openLogin: (site: string, url?: string, label?: string) => invoke<void>('open_login', { site, url, label }),
  saveLoginCookies: (site: string, label?: string) => invoke<AccountSummary[]>('save_login_cookies', { site, label }),
  listAccounts: () => invoke<AccountSummary[]>('list_accounts'),
  importCookiesFile: (path: string, label?: string) => invoke<AccountSummary[]>('import_cookies_file', { path, label }),
  importCookiesText: (site: string, text: string, label?: string) => invoke<AccountSummary[]>('import_cookies_text', { site, text, label }),
  renameAccount: (id: string, label: string) => invoke<AccountSummary[]>('rename_account', { id, label }),
  setDefaultAccount: (id: string) => invoke<AccountSummary[]>('set_default_account', { id }),
  deleteAccount: (id: string) => invoke<AccountSummary[]>('delete_account', { id }),
  checkAccount: (id: string) => invoke<AccountStatus>('check_account', { id }),
  routeSpeedtest: (url: string) => invoke<RouteSpeed[]>('route_speedtest', { url }),
  testRoute: (url: string) => invoke<RouteTest>('test_route', { url }),
}
