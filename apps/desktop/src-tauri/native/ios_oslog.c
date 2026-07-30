/* Tier 3 P0-1: native OSLog shim (iOS only).
 *
 * Compile-time format strings only — never pass a Rust-owned string as the
 * format itself. Numbers use %{public}llu; allowlisted one-line messages use
 * %{public}s. Do NOT log whole records as %{public}@ (leaks paths / panic text).
 *
 * Linked only when TARGET contains "ios" (see build.rs).
 */
#include <os/log.h>
#include <stdint.h>

enum {
  PKB_OSLOG_DEFAULT = 0x00, /* OS_LOG_TYPE_DEFAULT — persisted evidence */
  PKB_OSLOG_INFO = 0x01,    /* OS_LOG_TYPE_INFO */
  PKB_OSLOG_DEBUG = 0x02,   /* OS_LOG_TYPE_DEBUG — not persisted by Apple */
  PKB_OSLOG_ERROR = 0x10,   /* OS_LOG_TYPE_ERROR */
  PKB_OSLOG_FAULT = 0x11    /* OS_LOG_TYPE_FAULT */
};

static os_log_type_t pkb_map_type(uint8_t t) {
  switch (t) {
  case PKB_OSLOG_INFO:
    return OS_LOG_TYPE_INFO;
  case PKB_OSLOG_DEBUG:
    return OS_LOG_TYPE_DEBUG;
  case PKB_OSLOG_ERROR:
    return OS_LOG_TYPE_ERROR;
  case PKB_OSLOG_FAULT:
    return OS_LOG_TYPE_FAULT;
  case PKB_OSLOG_DEFAULT:
  default:
    return OS_LOG_TYPE_DEFAULT;
  }
}

void pkb_oslog_u64(const char *subsystem, const char *category, uint8_t type,
                   const char *label, uint64_t value) {
  os_log_t log = os_log_create(subsystem, category);
  os_log_with_type(log, pkb_map_type(type), "%{public}s=%{public}llu", label,
                   (unsigned long long)value);
}

void pkb_oslog_msg(const char *subsystem, const char *category, uint8_t type,
                   const char *msg) {
  os_log_t log = os_log_create(subsystem, category);
  /* Allowlisted, already-sanitized one-liner only. */
  os_log_with_type(log, pkb_map_type(type), "%{public}s", msg);
}
