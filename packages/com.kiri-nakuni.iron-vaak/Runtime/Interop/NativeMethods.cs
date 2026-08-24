using System.Runtime.InteropServices;

namespace IronVaak.Interop
{
    internal static unsafe class NativeMethods
    {
#if UNITY_IOS && !UNITY_EDITOR
        private const string LibraryName = "__Internal";
#else
        private const string LibraryName = "iron_vaak_native";
#endif

        private const CallingConvention Convention = CallingConvention.Cdecl;

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_abi_info(
            out NativeAbiInfo info,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_context_create(
            out ulong context,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_context_destroy(
            ulong context,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_prepare(
            ulong context,
            byte* source,
            ulong sourceLength,
            NativeHostLayoutEntry* entries,
            ulong entryCount,
            byte* names,
            ulong namesLength,
            out ulong prepared,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_prepared_destroy(
            ulong context,
            ulong prepared,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_create(
            ulong context,
            ulong prepared,
            out ulong runner,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_destroy(
            ulong context,
            ulong runner,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_run(
            ulong context,
            ulong runner,
            byte* snapshot,
            ulong snapshotLength,
            byte* runId,
            byte* transactionId,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_report_info(
            ulong context,
            ulong runner,
            out NativeRunnerReportInfo info,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_report_patch_copy(
            ulong context,
            ulong runner,
            byte* output,
            ulong capacity,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_report_top_level_copy(
            ulong context,
            ulong runner,
            byte* output,
            ulong capacity,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_report_diagnostic_copy(
            ulong context,
            ulong runner,
            ulong index,
            out NativeDiagnosticEnvelope diagnostic,
            byte* outputMessage,
            ulong messageCapacity,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_runner_clear_report(
            ulong context,
            ulong runner,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_diagnostics_count(
            ulong context,
            ulong diagnostics,
            out ulong count,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_diagnostic_copy(
            ulong context,
            ulong diagnostics,
            ulong index,
            out NativeDiagnosticEnvelope diagnostic,
            byte* outputMessage,
            ulong messageCapacity,
            out NativeCallEnvelope envelope);

        [DllImport(LibraryName, CallingConvention = Convention, ExactSpelling = true)]
        internal static extern uint iron_vaak_v0_diagnostics_destroy(
            ulong context,
            ulong diagnostics,
            out NativeCallEnvelope envelope);
    }
}
