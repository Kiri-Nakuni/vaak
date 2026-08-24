#ifndef IRON_VAAK_V0_H
#define IRON_VAAK_V0_H

/*
 * IRON VAAK v0 fixed-width record fixture.
 *
 * The raw-pointer exports are implemented by the separate iron-vaak-native
 * crate. The safe core remains free of unsafe code.
 */

#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#if defined(IRON_VAAK_BUILDING_DLL)
#define IRON_VAAK_API __declspec(dllexport)
#else
#define IRON_VAAK_API __declspec(dllimport)
#endif
#elif defined(__GNUC__) || defined(__clang__)
#define IRON_VAAK_API __attribute__((visibility("default")))
#else
#define IRON_VAAK_API
#endif

#if defined(__cplusplus)
#define IRON_VAAK_STATIC_ASSERT(condition, message) static_assert(condition, message)
#else
#define IRON_VAAK_STATIC_ASSERT(condition, message) _Static_assert(condition, message)
#endif

#define IRON_VAAK_V0_STATUS_OK UINT32_C(0)
#define IRON_VAAK_V0_STATUS_INVALID_ARGUMENT UINT32_C(1)
#define IRON_VAAK_V0_STATUS_ABI_MISMATCH UINT32_C(2)
#define IRON_VAAK_V0_STATUS_MALFORMED_WIRE UINT32_C(3)
#define IRON_VAAK_V0_STATUS_LIMIT_EXCEEDED UINT32_C(4)
#define IRON_VAAK_V0_STATUS_STALE_HANDLE UINT32_C(5)
#define IRON_VAAK_V0_STATUS_BUSY UINT32_C(6)
#define IRON_VAAK_V0_STATUS_POISONED UINT32_C(7)
#define IRON_VAAK_V0_STATUS_BUFFER_TOO_SMALL UINT32_C(8)
#define IRON_VAAK_V0_STATUS_INTERNAL_PANIC UINT32_C(9)

#define IRON_VAAK_V0_FEATURE_PREPARE_RUN_MANY (UINT64_C(1) << 0)
#define IRON_VAAK_V0_FEATURE_SNAPSHOT_PATCH (UINT64_C(1) << 1)
#define IRON_VAAK_V0_FEATURE_DIAGNOSTIC_COPY (UINT64_C(1) << 2)
#define IRON_VAAK_V0_FEATURE_THREAD_SAFE_HANDLES (UINT64_C(1) << 3)

#define IRON_VAAK_V0_PROGRAM_COMPLETED UINT16_C(1)
#define IRON_VAAK_V0_PROGRAM_ERROR UINT16_C(2)
#define IRON_VAAK_V0_PROGRAM_HOST_CONTRACT_ERROR UINT16_C(3)
#define IRON_VAAK_V0_PROGRAM_INTERNAL_PANIC UINT16_C(4)

#define IRON_VAAK_V0_TOP_LEVEL_NONE UINT16_C(0)
#define IRON_VAAK_V0_TOP_LEVEL_AKASHA UINT16_C(1)
#define IRON_VAAK_V0_TOP_LEVEL_VALUE UINT16_C(2)
#define IRON_VAAK_V0_TOP_LEVEL_PARADOX UINT16_C(3)
#define IRON_VAAK_V0_TOP_LEVEL_ESCAPE UINT16_C(4)
#define IRON_VAAK_V0_TOP_LEVEL_UNSUPPORTED_AGGREGATE UINT16_C(5)

typedef uint64_t IronVaakContextTokenV0;
typedef uint64_t IronVaakPreparedTokenV0;
typedef uint64_t IronVaakRunnerTokenV0;
typedef uint64_t IronVaakDiagnosticsTokenV0;

typedef struct IronVaakAbiInfoV0 {
    uint32_t struct_size;
    uint16_t abi_major;
    uint16_t abi_minor;
    uint64_t supported_features;
    uint64_t required_alignment;
    uint64_t max_wire_bytes;
    uint64_t reserved[4];
} IronVaakAbiInfoV0;

typedef struct IronVaakCallEnvelopeV0 {
    uint32_t struct_size;
    uint32_t transport_status;
    uint32_t detail_code;
    uint32_t flags;
    uint64_t required_bytes;
    uint64_t written_bytes;
    uint64_t diagnostic_id;
    uint64_t reserved[3];
} IronVaakCallEnvelopeV0;

typedef struct IronVaakDiagnosticEnvelopeV0 {
    uint32_t struct_size;
    uint16_t severity;
    uint16_t reserved0;
    uint32_t origin;
    uint32_t stable_code;
    uint64_t span_start;
    uint64_t span_length;
    uint64_t message_offset;
    uint64_t message_length;
    uint64_t incident_id;
    uint64_t reserved1;
} IronVaakDiagnosticEnvelopeV0;

typedef struct IronVaakHostLayoutEntryV0 {
    uint32_t struct_size;
    uint32_t flags;
    uint32_t slot_index;
    uint32_t value_type;
    uint64_t entity_id;
    uint32_t property_id;
    uint32_t capability_table_index;
    uint64_t name_offset;
    uint64_t name_length;
    uint64_t reserved1;
} IronVaakHostLayoutEntryV0;

typedef struct IronVaakValueNodeV0 {
    uint32_t type_tag;
    uint32_t type_id_or_flags;
    uint64_t scalar_bits_or_payload_offset;
    uint64_t payload_length_or_first_child;
    uint64_t child_count_or_reserved;
} IronVaakValueNodeV0;

typedef struct IronVaakSnapshotRecordV0 {
    uint64_t entity_id;
    uint32_t property_id;
    uint32_t type_tag;
    uint64_t property_revision;
    uint64_t value_node_index_or_scalar;
    uint64_t payload_aux;
    uint32_t flags;
    uint32_t reserved;
} IronVaakSnapshotRecordV0;

typedef struct IronVaakPatchRecordV0 {
    uint64_t entity_id;
    uint32_t property_id;
    uint16_t operation;
    uint16_t flags;
    uint64_t expected_property_revision;
    uint32_t value_type_tag;
    uint32_t capability_table_index;
    uint64_t value_node_index_or_payload_offset;
    uint64_t value_aux_or_length;
} IronVaakPatchRecordV0;

typedef struct IronVaakRunnerReportInfoV0 {
    uint32_t struct_size;
    uint16_t program_status;
    uint16_t top_level_kind;
    uint32_t top_level_value_type;
    uint32_t flags;
    uint64_t top_level_scalar_bits;
    uint64_t top_level_span_start;
    uint64_t top_level_span_length;
    uint64_t patch_bytes;
    uint64_t top_level_bytes;
    uint64_t diagnostic_count;
} IronVaakRunnerReportInfoV0;

IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakContextTokenV0) == 8, "context token width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakPreparedTokenV0) == 8, "prepared token width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakRunnerTokenV0) == 8, "runner token width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakDiagnosticsTokenV0) == 8, "diagnostics token width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakAbiInfoV0) == 64, "ABI info width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakCallEnvelopeV0) == 64, "call envelope width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakDiagnosticEnvelopeV0) == 64, "diagnostic width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakHostLayoutEntryV0) == 56, "host layout width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakValueNodeV0) == 32, "value node width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakSnapshotRecordV0) == 48, "snapshot record width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakPatchRecordV0) == 48, "patch record width");
IRON_VAAK_STATIC_ASSERT(sizeof(IronVaakRunnerReportInfoV0) == 64, "runner report info width");
IRON_VAAK_STATIC_ASSERT(
    offsetof(IronVaakHostLayoutEntryV0, entity_id) == 16,
    "host layout entity offset");
IRON_VAAK_STATIC_ASSERT(
    offsetof(IronVaakHostLayoutEntryV0, name_offset) == 32,
    "host layout name offset");
IRON_VAAK_STATIC_ASSERT(
    offsetof(IronVaakSnapshotRecordV0, property_revision) == 16,
    "snapshot revision offset");
IRON_VAAK_STATIC_ASSERT(
    offsetof(IronVaakPatchRecordV0, expected_property_revision) == 16,
    "patch revision offset");

#if defined(__cplusplus)
extern "C" {
#endif

IRON_VAAK_API uint32_t iron_vaak_v0_abi_info(
    IronVaakAbiInfoV0 *output_info,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_context_create(
    IronVaakContextTokenV0 *output_context,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_context_destroy(
    IronVaakContextTokenV0 context,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_prepare(
    IronVaakContextTokenV0 context,
    const uint8_t *source,
    uint64_t source_length,
    const IronVaakHostLayoutEntryV0 *entries,
    uint64_t entry_count,
    const uint8_t *names,
    uint64_t names_length,
    IronVaakPreparedTokenV0 *output_prepared,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_prepared_destroy(
    IronVaakContextTokenV0 context,
    IronVaakPreparedTokenV0 prepared,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_create(
    IronVaakContextTokenV0 context,
    IronVaakPreparedTokenV0 prepared,
    IronVaakRunnerTokenV0 *output_runner,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_destroy(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_run(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    const uint8_t *snapshot,
    uint64_t snapshot_length,
    const uint8_t run_id[16],
    const uint8_t transaction_id[16],
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_report_info(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    IronVaakRunnerReportInfoV0 *output_info,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_report_patch_copy(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    uint8_t *output,
    uint64_t capacity,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_report_top_level_copy(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    uint8_t *output,
    uint64_t capacity,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_report_diagnostic_copy(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    uint64_t index,
    IronVaakDiagnosticEnvelopeV0 *output_diagnostic,
    uint8_t *output_message,
    uint64_t message_capacity,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_runner_clear_report(
    IronVaakContextTokenV0 context,
    IronVaakRunnerTokenV0 runner,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_diagnostics_count(
    IronVaakContextTokenV0 context,
    IronVaakDiagnosticsTokenV0 diagnostics,
    uint64_t *output_count,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_diagnostic_copy(
    IronVaakContextTokenV0 context,
    IronVaakDiagnosticsTokenV0 diagnostics,
    uint64_t index,
    IronVaakDiagnosticEnvelopeV0 *output_diagnostic,
    uint8_t *output_message,
    uint64_t message_capacity,
    IronVaakCallEnvelopeV0 *output_envelope);

IRON_VAAK_API uint32_t iron_vaak_v0_diagnostics_destroy(
    IronVaakContextTokenV0 context,
    IronVaakDiagnosticsTokenV0 diagnostics,
    IronVaakCallEnvelopeV0 *output_envelope);

#if defined(__cplusplus)
}
#endif

#undef IRON_VAAK_STATIC_ASSERT
#undef IRON_VAAK_API

#endif /* IRON_VAAK_V0_H */
