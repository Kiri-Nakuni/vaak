using IronVaak.Scripting;
using System;
using System.Threading;
using System.Threading.Tasks;

namespace IronVaak.Unity
{
    /// <summary>
    /// Captures Unity's main-thread SynchronizationContext. Script calculation happens first;
    /// validation and commit are posted as a later main-thread turn, so apply never calls a runtime.
    /// </summary>
    public sealed class UnityPlanScheduler
    {
        private readonly ScriptPlanCoordinator _coordinator;
        private readonly SynchronizationContext _mainThread;

        public UnityPlanScheduler(ScriptPlanCoordinator coordinator, SynchronizationContext? mainThread = null)
        {
            _coordinator = coordinator ?? throw new ArgumentNullException(nameof(coordinator));
            _mainThread = mainThread ?? SynchronizationContext.Current ??
                throw new InvalidOperationException("Construct UnityPlanScheduler on the Unity main thread or pass its SynchronizationContext.");
        }

        public async ValueTask<ScriptCompositionResult> ExecuteAndApplyAsync<TValidated>(
            SettingsSnapshot snapshot,
            WireId128 runId,
            WireId128 transactionId,
            Func<SettingsPatch, TValidated> validateWithoutMutation,
            Action<TValidated> commit,
            CancellationToken cancellationToken = default)
        {
            if (validateWithoutMutation == null) throw new ArgumentNullException(nameof(validateWithoutMutation));
            if (commit == null) throw new ArgumentNullException(nameof(commit));
            ScriptCompositionResult result = await _coordinator
                .ExecuteAsync(snapshot, runId, transactionId, cancellationToken)
                .ConfigureAwait(false);
            await PostApply(result.Patch, validateWithoutMutation, commit, cancellationToken).ConfigureAwait(false);
            return result;
        }

        private Task PostApply<TValidated>(
            SettingsPatch patch,
            Func<SettingsPatch, TValidated> validate,
            Action<TValidated> commit,
            CancellationToken cancellationToken)
        {
            var completion = new TaskCompletionSource<bool>(TaskCreationOptions.RunContinuationsAsynchronously);
            _mainThread.Post(_ =>
            {
                try
                {
                    cancellationToken.ThrowIfCancellationRequested();
                    TValidated validated = validate(patch);
                    cancellationToken.ThrowIfCancellationRequested();
                    commit(validated);
                    completion.SetResult(true);
                }
                catch (Exception error)
                {
                    completion.SetException(error);
                }
            }, null);
            return completion.Task;
        }
    }
}
