using Microsoft.Win32.SafeHandles;
using System;

namespace IronVaak.Interop
{
    internal abstract class TokenSafeHandle : SafeHandleZeroOrMinusOneIsInvalid
    {
        protected TokenSafeHandle() : base(true)
        {
            if (IntPtr.Size != 8)
            {
                throw new PlatformNotSupportedException("IRON VAAK v0 managed facade requires a 64-bit process.");
            }
        }

        protected TokenSafeHandle(ulong token) : this()
        {
            SetHandle(new IntPtr(unchecked((long)token)));
        }

        internal ulong Token => unchecked((ulong)handle.ToInt64());
    }

    internal sealed class ContextSafeHandle : TokenSafeHandle
    {
        internal ContextSafeHandle(ulong token) : base(token) { }

        protected override bool ReleaseHandle()
        {
            uint status = NativeMethods.iron_vaak_v0_context_destroy(Token, out _);
            return status == (uint)TransportStatus.Ok;
        }
    }

    internal abstract class ChildSafeHandle : TokenSafeHandle
    {
        private readonly ContextSafeHandle _context;
        private bool _contextReference;

        protected ChildSafeHandle(ContextSafeHandle context, ulong token) : base(token)
        {
            _context = context ?? throw new ArgumentNullException(nameof(context));
            bool success = false;
            _context.DangerousAddRef(ref success);
            _contextReference = success;
        }

        protected ulong ContextToken => _context.Token;

        protected void ReleaseContext()
        {
            if (_contextReference)
            {
                _contextReference = false;
                _context.DangerousRelease();
            }
        }
    }

    internal sealed class PreparedSafeHandle : ChildSafeHandle
    {
        internal PreparedSafeHandle(ContextSafeHandle context, ulong token) : base(context, token) { }

        protected override bool ReleaseHandle()
        {
            uint status = NativeMethods.iron_vaak_v0_prepared_destroy(ContextToken, Token, out _);
            ReleaseContext();
            return status == (uint)TransportStatus.Ok || status == (uint)TransportStatus.StaleHandle;
        }
    }

    internal sealed class RunnerSafeHandle : ChildSafeHandle
    {
        internal RunnerSafeHandle(ContextSafeHandle context, ulong token) : base(context, token) { }

        protected override bool ReleaseHandle()
        {
            uint status = NativeMethods.iron_vaak_v0_runner_destroy(ContextToken, Token, out _);
            ReleaseContext();
            return status == (uint)TransportStatus.Ok || status == (uint)TransportStatus.StaleHandle;
        }
    }
}
