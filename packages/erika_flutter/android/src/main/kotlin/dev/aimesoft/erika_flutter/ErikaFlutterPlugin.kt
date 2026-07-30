package dev.aimesoft.erika_flutter

import android.content.Context
import android.content.res.AssetFileDescriptor
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.ParcelFileDescriptor
import android.os.SystemClock
import android.system.ErrnoException
import android.system.Os
import android.system.OsConstants
import android.util.Log
import android.view.Choreographer
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.LifecycleOwner
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.embedding.engine.plugins.lifecycle.FlutterLifecycleAdapter
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.EOFException
import java.io.File
import java.io.FileDescriptor
import java.io.FileNotFoundException
import java.io.FileOutputStream
import java.io.IOException
import java.io.InputStream
import java.util.concurrent.CancellationException
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.Future
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.max

class ErikaFlutterPlugin :
    FlutterPlugin,
    ActivityAware,
    LifecycleEventObserver,
    MethodChannel.MethodCallHandler,
    EventChannel.StreamHandler {
    private lateinit var applicationContext: Context
    private lateinit var methodChannel: MethodChannel
    private lateinit var eventChannel: EventChannel
    private lateinit var choreographer: Choreographer
    private lateinit var audioFocus: ErikaAudioFocus
    private lateinit var mediaSession: ErikaMediaSession
    private lateinit var mainHandler: Handler
    private lateinit var contentPreparationExecutor: ExecutorService
    @Volatile
    private var contentSpoolScavengeFuture: Future<*>? = null
    private val players = linkedMapOf<Long, AndroidPlayerHost>()
    private val videoViews = linkedMapOf<Int, ErikaAndroidVideoView>()
    private var eventSink: EventChannel.EventSink? = null
    private var frameScheduled = false
    private var attachedToEngine = false
    private var activityLifecycle: Lifecycle? = null
    private var activityActive = false
    private var activeMediaPlayerId: Long? = null

    internal val isActivityActive: Boolean
        get() = attachedToEngine && activityActive

    private val frameCallback = Choreographer.FrameCallback { frameTimeNanos ->
        frameScheduled = false
        if (!isActivityActive) {
            return@FrameCallback
        }
        val tickingPlayers = players.values.filter(AndroidPlayerHost::shouldTick)
        if (tickingPlayers.isEmpty()) {
            return@FrameCallback
        }
        val timeSeconds = frameTimeNanos.toDouble() / 1_000_000_000.0
        tickingPlayers.forEach { host ->
            try {
                runCatching { host.renderTick(timeSeconds) }
                    .onSuccess { response -> reportRenderResponse(host, response) }
                    .onFailure { error -> reportRenderException(host, error) }
            } finally {
                host.markRenderAttempted()
            }
        }
        players.values.toList().forEach(::drainEvents)
        refreshFrameScheduling()
    }

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        applicationContext = binding.applicationContext
        choreographer = Choreographer.getInstance()
        mainHandler = Handler(Looper.getMainLooper())
        contentPreparationExecutor = newContentPreparationExecutor()
        contentSpoolScavengeFuture = scheduleContentSpoolStartupScavenge()
        audioFocus = ErikaAudioFocus(
            applicationContext,
            onFocusLoss = ::handleAudioFocusLoss,
            onFocusGain = ::handleAudioFocusGain,
        )
        mediaSession = ErikaMediaSession(
            applicationContext,
            object : ErikaMediaCommandHandler {
                override fun play(playerId: Long) = performSystemMediaCommand(playerId, "play")
                override fun pause(playerId: Long) = performSystemMediaCommand(playerId, "pause")
                override fun stop(playerId: Long) = performSystemMediaCommand(playerId, "stop")
                override fun seek(playerId: Long, positionMicros: Long) =
                    performSystemMediaCommand(playerId, "seek", mapOf("positionMicros" to positionMicros))
                override fun previous(playerId: Long) =
                    emitSystemMediaNavigation(playerId, SYSTEM_MEDIA_NAVIGATION_PREVIOUS)
                override fun next(playerId: Long) =
                    emitSystemMediaNavigation(playerId, SYSTEM_MEDIA_NAVIGATION_NEXT)
            },
        )
        ErikaMediaCommandReceiver.commandHandler = mediaSession::dispatch
        ErikaMediaPlaybackService.tickHandler = ::performBackgroundPlaybackTick
        methodChannel = MethodChannel(binding.binaryMessenger, PLAYER_CHANNEL)
        eventChannel = EventChannel(binding.binaryMessenger, EVENT_CHANNEL)
        methodChannel.setMethodCallHandler(this)
        eventChannel.setStreamHandler(this)
        binding.platformViewRegistry.registerViewFactory(
            VIDEO_VIEW_TYPE,
            ErikaAndroidVideoViewFactory(this),
        )
        binding.platformViewRegistry.registerViewFactory(
            HDR_VIDEO_VIEW_TYPE,
            ErikaAndroidVideoViewFactory(this, useHdrSurface = true),
        )
        attachedToEngine = true
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        detachFromActivity()
        attachedToEngine = false
        cancelFrameCallback()
        methodChannel.setMethodCallHandler(null)
        eventChannel.setStreamHandler(null)
        eventSink = null
        videoViews.values.toList().forEach(ErikaAndroidVideoView::dispose)
        videoViews.clear()
        players.values.toList().forEach(::destroyPlayer)
        players.clear()
        if (::contentPreparationExecutor.isInitialized) {
            contentPreparationExecutor.shutdownNow()
        }
        audioFocus.abandon()
        ErikaMediaCommandReceiver.commandHandler = null
        ErikaMediaPlaybackService.tickHandler = null
        mediaSession.release()
    }

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        attachToActivity(binding)
    }

    override fun onDetachedFromActivityForConfigChanges() {
        detachFromActivity()
    }

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
        attachToActivity(binding)
    }

    override fun onDetachedFromActivity() {
        detachFromActivity()
    }

    override fun onStateChanged(source: LifecycleOwner, event: Lifecycle.Event) {
        val lifecycle = activityLifecycle
        if (lifecycle == null || source.lifecycle !== lifecycle) {
            return
        }
        val active = androidActivityActiveForEvent(event) ?: return
        Log.i(
            TAG,
            "activityLifecycleEvent event=$event state=${lifecycle.currentState} active=$active",
        )
        setActivityActive(active)
    }

    override fun onListen(arguments: Any?, events: EventChannel.EventSink) {
        eventSink = events
        players.values.forEach(::drainEvents)
        refreshFrameScheduling()
    }

    override fun onCancel(arguments: Any?) {
        eventSink = null
    }

    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        try {
            when (call.method) {
                "create" -> createPlayer(arguments(call), result)
                "dispose" -> disposePlayer(arguments(call), result)
                "attachView" -> attachView(arguments(call), result)
                "detachView" -> detachView(arguments(call), result)
                "attachOverlay" -> attachOverlay(arguments(call), result)
                "detachOverlay" -> detachOverlay(arguments(call), result)
                "setOverlayFrame" -> setOverlayFrame(arguments(call), result)
                "screenshot" -> captureFrame(arguments(call), result)
                "setMediaMetadata" -> setMediaMetadata(arguments(call), result)
                "setSystemMediaNavigation" -> setSystemMediaNavigation(arguments(call), result)
                in NATIVE_METHODS -> invokePlayer(call.method, arguments(call), result)
                else -> result.notImplemented()
            }
        } catch (error: Throwable) {
            Log.e(TAG, "Method ${call.method} failed", error)
            result.error(
                "ERIKA_ERROR",
                error.message ?: "Erika Android method ${call.method} failed",
                null,
            )
        }
    }

    internal fun registerVideoView(view: ErikaAndroidVideoView) {
        videoViews.put(view.viewId, view)?.takeIf { it !== view }?.dispose()
    }

    internal fun unregisterVideoView(view: ErikaAndroidVideoView) {
        if (videoViews[view.viewId] === view) {
            videoViews.remove(view.viewId)
        }
    }

    internal fun onPlayerRenderStateChanged() {
        refreshFrameScheduling()
    }

    internal fun reportSurfaceResponse(
        host: AndroidPlayerHost,
        operation: String,
        response: NativeResponse,
    ) {
        if (response.ok) {
            host.lastSurfaceError = null
        } else {
            val signature = "$operation:${response.status}:${response.error.orEmpty()}"
            Log.e(
                TAG,
                "$operation failed for player ${host.handle}: status=${response.status} ${response.error.orEmpty()}",
            )
            if (host.lastSurfaceError != signature) {
                host.lastSurfaceError = signature
                enqueueHostError(
                    host,
                    operation,
                    response.status,
                    response.error ?: "$operation failed",
                )
            }
        }
        refreshFrameScheduling()
    }

    internal fun reportSurfaceRecoveryExhausted(
        host: AndroidPlayerHost,
        viewId: Int,
        operation: String,
        generation: Long,
        retryAttempts: Int,
        response: NativeResponse,
    ) {
        val failedAttempts = retryAttempts + 1
        val error = response.error ?: "$operation failed without a native error"
        Log.e(
            TAG,
            "surfaceRecoveryExhausted playerId=${host.handle} viewId=$viewId " +
                "operation=$operation generation=$generation " +
                "failedAttempts=$failedAttempts retryAttempts=$retryAttempts " +
                "status=${response.status} error=$error",
        )
        enqueueHostError(
            host,
            "surfaceRecovery",
            response.status,
            "$operation recovery exhausted after $failedAttempts failed attempts: $error",
            mapOf(
                "surfaceOperation" to operation,
                "surfaceViewId" to viewId,
                "surfaceRecoveryGeneration" to generation,
                "surfaceRecoveryFailedAttempts" to failedAttempts,
                "surfaceRecoveryRetryAttempts" to retryAttempts,
            ),
        )
    }

    private fun createPlayer(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val outputMode = arguments.int("outputMode") ?: 0
        val defaultHeadroom = if (outputMode == 2) 4f else 1f
        val edrHeadroom =
            (arguments.number("edrHeadroom")?.toFloat() ?: defaultHeadroom).coerceAtLeast(1f)
        val upscaler = arguments.int("upscaler") ?: arguments.int("lumaUpscaler") ?: 0
        val handle = ErikaNative.nativeCreate(outputMode, edrHeadroom, upscaler)
        if (handle == 0L) {
            val reason = runCatching(ErikaNative::nativeLastError)
                .getOrNull()
                .orEmpty()
                .ifBlank { "Erika C ABI did not provide a presenter creation error" }
            Log.e(TAG, "Erika Android presenter creation failed: $reason")
            result.error(
                "ERIKA_ERROR",
                "Erika Android presenter creation failed: $reason",
                mapOf("stage" to "presenter_create", "reason" to reason),
            )
            return
        }
        players[handle] = AndroidPlayerHost(
            handle,
            outputMode,
            arguments["allowBackgroundPlayback"] == true,
        )
        result.success(handle)
    }

    private fun disposePlayer(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        players.remove(host.handle)
        destroyPlayer(host)
        result.success(null)
    }

    private fun destroyPlayer(host: AndroidPlayerHost) {
        host.cancelPlaybackIntent()
        host.cancelContentPreparations("player_disposed")
        abandonAudioFocusIfIdle()
        runCatching(host::destroy).onFailure { error ->
            Log.e(TAG, "Unable to destroy Erika player ${host.handle}", error)
        }
        if (activeMediaPlayerId == host.handle) {
            activeMediaPlayerId = null
            mediaSession.clear(host.handle)
        }
        refreshFrameScheduling()
    }

    private fun attachView(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        val viewId = arguments.requiredInt("viewId")
        val view = videoViews[viewId]
        if (view == null) {
            result.error("ERIKA_ERROR", "Erika Android video view $viewId was not found", null)
            return
        }
        if (view.isExtendedLinearSurface != host.requiresExtendedLinearSurface) {
            result.error(
                "ERIKA_ERROR",
                "Erika Android player ${host.handle} requires " +
                    if (host.requiresExtendedLinearSurface) {
                        "an extended-linear SurfaceView"
                    } else {
                        "an SDR TextureView"
                    },
                null,
            )
            return
        }
        complete(result, view.bind(host))
    }

    private fun detachView(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        val viewId = arguments.requiredInt("viewId")
        val view = videoViews[viewId]
        if (view != null && host.attachedView === view) {
            complete(result, view.unbind(host))
        } else {
            result.success(null)
        }
    }

    private fun attachOverlay(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        val view = host.attachedView
            ?: videoViews.values.lastOrNull { candidate ->
                candidate.isExtendedLinearSurface == host.requiresExtendedLinearSurface &&
                    players.values.none { it.attachedView === candidate }
            }
        if (view == null) {
            result.error(
                "ERIKA_ERROR",
                "Android window-overlay playback requires an Erika TextureView platform view",
                null,
            )
            return
        }
        val response = view.bind(host)
        if (response.ok) {
            result.success(view.viewId)
        } else {
            complete(result, response)
        }
    }

    private fun detachOverlay(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        val view = host.attachedView
        if (view == null) {
            result.success(null)
            return
        }
        complete(result, view.unbind(host))
    }

    private fun setOverlayFrame(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        val visible = arguments["visible"] as? Boolean ?: true
        val debugLabel = arguments["debugLabel"] as? String
        val requestedViewId = arguments.int("viewId")?.takeIf { it >= 0 }
        val view = if (requestedViewId != null) {
            val requestedView = videoViews[requestedViewId]
            if (requestedView == null) {
                result.error(
                    "ERIKA_ERROR",
                    "Erika Android video view $requestedViewId was not found",
                    mapOf("stage" to "setOverlayFrame", "viewId" to requestedViewId),
                )
                return
            }
            if (host.attachedView !== requestedView) {
                result.error(
                    "ERIKA_ERROR",
                    "Erika Android video view $requestedViewId is not attached to player ${host.handle}",
                    mapOf("stage" to "setOverlayFrame", "viewId" to requestedViewId),
                )
                return
            }
            requestedView
        } else {
            host.attachedView
        }
        if (view == null) {
            if (!visible) {
                result.success(null)
                return
            }
            result.error(
                "ERIKA_ERROR",
                "Erika Android player ${host.handle} has no attached video view",
                mapOf("stage" to "setOverlayFrame"),
            )
            return
        }
        view.setFlutterManagedVisibility(visible, debugLabel)
        result.success(null)
    }

    private fun captureFrame(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        val requestedViewId = arguments.int("viewId")
        val view = if (requestedViewId != null) {
            val requestedView = videoViews[requestedViewId]
            if (requestedView == null) {
                result.error(
                    "ERIKA_ERROR",
                    "Erika Android video view $requestedViewId was not found",
                    mapOf("stage" to "screenshot", "viewId" to requestedViewId),
                )
                return
            }
            if (host.attachedView !== requestedView) {
                result.error(
                    "ERIKA_ERROR",
                    "Erika Android video view $requestedViewId is not attached to player ${host.handle}",
                    mapOf("stage" to "screenshot", "viewId" to requestedViewId),
                )
                return
            }
            requestedView
        } else {
            host.attachedView
        }
        val width = arguments.int("width")?.takeIf { it > 0 }
            ?: view?.pixelWidth()?.takeIf { it > 0 }
        val height = arguments.int("height")?.takeIf { it > 0 }
            ?: view?.pixelHeight()?.takeIf { it > 0 }
        if (width == null || height == null) {
            result.error(
                "ERIKA_ERROR",
                "Screenshot width and height are required before an Android video surface is attached",
                null,
            )
            return
        }
        result.success(host.captureFrame(width, height))
    }

    private fun invokePlayer(
        method: String,
        arguments: Map<String, Any?>,
        result: MethodChannel.Result,
    ) {
        val host = player(arguments)
        if (method == "open") {
            (arguments["metadata"] as? Map<*, *>)?.let {
                host.setMediaMetadata(androidMediaMetadata(arguments))
            }
        }
        if (method == "play") {
            playWithAudioFocus(host, result)
            return
        }

        if (method in PLAYBACK_INTENT_CANCEL_METHODS) {
            host.cancelPlaybackIntent()
            abandonAudioFocusIfIdle()
            refreshFrameScheduling()
        }

        if (method in CONTENT_PREPARATION_INVALIDATION_METHODS) {
            host.cancelContentPreparations("superseded_by_$method")
        }

        if (requiresAsyncContentPreparation(method, arguments)) {
            invokePlayerAfterContentPreparation(host, method, arguments, result)
            return
        }

        val prepared = prepareNativeArguments(method, arguments)
        invokePreparedPlayer(host, method, prepared, result)
    }

    private fun invokePreparedPlayer(
        host: AndroidPlayerHost,
        method: String,
        prepared: PreparedNativeArguments,
        result: MethodChannel.Result,
    ) {
        val argumentsJson = try {
            NativeJson.encodeArguments(prepared.arguments)
        } catch (error: Throwable) {
            prepared.detachedFd?.let(::closeDetachedFileDescriptor)
            throw error
        }
        // Once nativeInvoke is entered, Rust owns every detached fd regardless of
        // the returned status and closes it either in the JNI bridge or source Drop.
        val response = try {
            host.invokeEncoded(method, argumentsJson, prepared.detachedFd ?: NO_OWNED_FD)
        } catch (error: UnsatisfiedLinkError) {
            // Native dispatch never began, so ownership is still on the Kotlin side.
            prepared.detachedFd?.let(::closeDetachedFileDescriptor)
            throw error
        }
        if (response.ok && method in RENDER_REQUEST_METHODS) {
            host.requestRender()
        }
        if (response.ok && method == "setPlaybackRate") {
            host.setPlaybackRate((prepared.arguments["rate"] as? Number)?.toFloat() ?: 1f)
            if (activeMediaPlayerId == host.handle) {
                mediaSession.update(host.mediaState)
            }
        }
        drainEvents(host)
        refreshFrameScheduling()
        complete(result, response)
    }

    private fun requiresAsyncContentPreparation(
        method: String,
        arguments: Map<String, Any?>,
    ): Boolean {
        if (method !in URI_METHODS) {
            return false
        }
        val uri = arguments["uri"] as? String ?: return false
        return uri.startsWith("content://", ignoreCase = true)
    }

    private fun invokePlayerAfterContentPreparation(
        host: AndroidPlayerHost,
        method: String,
        arguments: Map<String, Any?>,
        result: MethodChannel.Result,
    ) {
        val rawUri = arguments["uri"] as? String
            ?: throw IllegalArgumentException("uri is required")
        val cancellation = AndroidContentPreparationCancellation()
        val command = PendingContentCommand(
            host = host,
            method = method,
            authority = Uri.parse(rawUri).authority,
            result = result,
            cancellation = cancellation,
        )
        command.token = host.beginContentPreparation { reason ->
            cancelPendingContentCommand(command, reason)
        }

        val future = try {
            contentPreparationExecutor.submit {
                val prepared = runCatching {
                    prepareNativeArguments(method, arguments, cancellation)
                }
                val posted = mainHandler.post {
                    finishContentPreparation(command, prepared)
                }
                if (!posted) {
                    prepared.getOrNull()?.detachedFd?.let(::closeDetachedFileDescriptor)
                    cancellation.cancel()
                }
            }
        } catch (error: RejectedExecutionException) {
            host.finishContentPreparation(command.token)
            if (command.claimCompletion()) {
                cancellation.cancel()
                Log.e(TAG, "Android content preparation executor rejected $method", error)
                result.error(
                    "ERIKA_ERROR",
                    "Android content preparation executor is unavailable",
                    mapOf(
                        "stage" to "content_prepare",
                        "method" to method,
                        "reason" to "executor_unavailable",
                    ),
                )
            }
            return
        }
        cancellation.attachFuture(future)
    }

    private fun finishContentPreparation(
        command: PendingContentCommand,
        prepared: Result<PreparedNativeArguments>,
    ) {
        val current = command.host.finishContentPreparation(command.token)
        if (!current || !command.claimCompletion()) {
            prepared.getOrNull()?.detachedFd?.let(::closeDetachedFileDescriptor)
            return
        }
        val failure = prepared.exceptionOrNull()
        if (failure != null) {
            val reason = androidContentSourceFailureReason(failure)
            val cancelled = failure is AndroidContentPreparationCancelledException
            Log.e(
                TAG,
                androidContentSourceEvent(
                    stage = if (cancelled) "cancelled" else "failed",
                    authority = command.authority,
                    fields = linkedMapOf(
                        "mode" to "background_prepare",
                        "method" to command.method,
                        "playerId" to command.host.handle,
                        "reason" to reason,
                        "message" to (failure.message ?: failure.javaClass.simpleName),
                    ),
                ),
                failure,
            )
            command.result.error(
                if (cancelled) "ERIKA_CONTENT_CANCELLED" else "ERIKA_ERROR",
                failure.message ?: "Android content preparation failed",
                mapOf(
                    "stage" to "content_prepare",
                    "method" to command.method,
                    "reason" to reason,
                ),
            )
            return
        }

        try {
            invokePreparedPlayer(
                command.host,
                command.method,
                checkNotNull(prepared.getOrNull()),
                command.result,
            )
        } catch (error: Throwable) {
            Log.e(TAG, "Async Erika ${command.method} invocation failed", error)
            command.result.error(
                "ERIKA_ERROR",
                error.message ?: "Erika Android method ${command.method} failed",
                mapOf("stage" to "content_invoke", "method" to command.method),
            )
        }
    }

    private fun cancelPendingContentCommand(command: PendingContentCommand, reason: String) {
        command.cancellation.cancel()
        if (!command.claimCompletion()) {
            return
        }
        Log.w(
            TAG,
            androidContentSourceEvent(
                stage = "cancelled",
                authority = command.authority,
                fields = linkedMapOf(
                    "mode" to "background_prepare",
                    "method" to command.method,
                    "playerId" to command.host.handle,
                    "reason" to reason,
                ),
            ),
        )
        runCatching {
            command.result.error(
                "ERIKA_CONTENT_CANCELLED",
                "Android content preparation for ${command.method} was cancelled: $reason",
                mapOf(
                    "stage" to "content_prepare",
                    "method" to command.method,
                    "reason" to reason,
                ),
            )
        }.onFailure { error ->
            // The Flutter messenger may already be detached. One failed result
            // delivery must not abort registry invalidation for other players.
            Log.w(
                TAG,
                "Failed to deliver cancelled Android content result for " +
                    "player ${command.host.handle}, method ${command.method}",
                error,
            )
        }
    }

    private fun playWithAudioFocus(host: AndroidPlayerHost, result: MethodChannel.Result) {
        host.requestPlayback()
        refreshFrameScheduling()
        if (!host.mediaState.canPlay(isActivityActive)) {
            host.cancelPlaybackIntent()
            result.success(null)
            return
        }
        val focusGrant = try {
            audioFocus.request()
        } catch (error: Throwable) {
            host.cancelPlaybackIntent()
            abandonAudioFocusIfIdle()
            refreshFrameScheduling()
            throw error
        }
        when (focusGrant) {
            AudioFocusGrant.GRANTED -> {
                val response = try {
                    host.invoke("play", emptyMap())
                } catch (error: Throwable) {
                    host.cancelPlaybackIntent()
                    abandonAudioFocusIfIdle()
                    refreshFrameScheduling()
                    throw error
                }
                if (response.ok) {
                    host.playbackStarted()
                    activeMediaPlayerId = host.handle
                    mediaSession.update(host.mediaState.copy(playbackState = PLAYING_STATE))
                } else {
                    host.cancelPlaybackIntent()
                    abandonAudioFocusIfIdle()
                }
                drainEvents(host)
                refreshFrameScheduling()
                complete(result, response)
            }
            AudioFocusGrant.DELAYED -> result.success(null)
            AudioFocusGrant.DENIED -> {
                host.cancelPlaybackIntent()
                abandonAudioFocusIfIdle()
                refreshFrameScheduling()
                result.error("ERIKA_AUDIO_FOCUS", "Android audio focus request was denied", null)
            }
        }
    }

    private fun attachToActivity(binding: ActivityPluginBinding) {
        detachFromActivity()
        val lifecycle = try {
            FlutterLifecycleAdapter.getActivityLifecycle(binding)
        } catch (error: Throwable) {
            Log.e(
                TAG,
                "activityLifecycleAttachFailed activity=${binding.activity.javaClass.name} active=false",
                error,
            )
            setActivityActive(false)
            return
        }
        activityLifecycle = lifecycle
        lifecycle.addObserver(this)
        val active = androidActivityIsActive(lifecycle.currentState)
        Log.i(
            TAG,
            "activityLifecycleAttached activity=${binding.activity.javaClass.name} " +
                "activityIsLifecycleOwner=${binding.activity is LifecycleOwner} " +
                "state=${lifecycle.currentState} active=$active",
        )
        setActivityActive(active)
    }

    private fun detachFromActivity() {
        activityLifecycle?.let { lifecycle ->
            lifecycle.removeObserver(this)
            Log.i(TAG, "activityLifecycleDetached state=${lifecycle.currentState} active=false")
        }
        activityLifecycle = null
        setActivityActive(false)
    }

    private fun setActivityActive(active: Boolean) {
        if (activityActive == active) {
            refreshFrameScheduling()
            return
        }
        activityActive = active
        if (active) {
            resumeFromActivityStop()
        } else {
            suspendForActivityStop()
        }
    }

    private fun suspendForActivityStop() {
        cancelFrameCallback()
        val hostsToPause = players.values.toList().filter { host ->
            !host.mediaState.allowBackgroundPlayback && host.cancelPlaybackIntent()
        }
        if (players.values.none { host ->
                host.mediaState.allowBackgroundPlayback &&
                    host.playbackPhase != AndroidPlaybackPhase.PAUSED
            }
        ) {
            audioFocus.abandon()
        }
        hostsToPause.forEach { host ->
            runCatching { host.invoke("pause", emptyMap()) }
                .onSuccess { response ->
                    reportBackgroundCommand(host, "lifecycle", "pause", response)
                }
                .onFailure { error -> Log.e(TAG, "Lifecycle pause threw", error) }
        }
        videoViews.values.toList().forEach { view ->
            runCatching(view::suspendSurface)
                .onFailure { error -> Log.e(TAG, "Lifecycle surface detach threw", error) }
        }
        players.values.toList().forEach(::drainEvents)
        refreshFrameScheduling()
    }

    private fun resumeFromActivityStop() {
        videoViews.values.toList().forEach { view ->
            runCatching(view::resumeSurface)
                .onFailure { error -> Log.e(TAG, "Lifecycle surface attach threw", error) }
        }
        resumePendingPlayback()
        refreshFrameScheduling()
    }

    private fun resumePendingPlayback() {
        if (!isActivityActive) {
            return
        }
        val pendingHosts = players.values.toList()
            .filter { it.playbackPhase == AndroidPlaybackPhase.PENDING }
        if (pendingHosts.isEmpty()) {
            return
        }
        val focusGrant = try {
            audioFocus.request()
        } catch (error: Throwable) {
            pendingHosts.forEach { host -> host.cancelPlaybackIntent() }
            abandonAudioFocusIfIdle()
            Log.e(TAG, "Android audio focus request threw while resuming playback", error)
            return
        }
        when (focusGrant) {
            AudioFocusGrant.GRANTED -> {
                pendingHosts.forEach { host ->
                    startPendingPlayback(host, "lifecycle")
                }
            }
            AudioFocusGrant.DELAYED -> Unit
            AudioFocusGrant.DENIED -> {
                pendingHosts.forEach { host -> host.cancelPlaybackIntent() }
                abandonAudioFocusIfIdle()
                Log.w(TAG, "Android audio focus denied while resuming Erika playback")
            }
        }
    }

    private fun startPendingPlayback(host: AndroidPlayerHost, source: String) {
        if (!host.mediaState.canPlay(isActivityActive) ||
            !audioFocus.focusGranted ||
            host.playbackPhase != AndroidPlaybackPhase.PENDING
        ) {
            return
        }
        val response = runCatching { host.invoke("play", emptyMap()) }
            .getOrElse { error ->
                host.cancelPlaybackIntent()
                abandonAudioFocusIfIdle()
                Log.e(TAG, "$source resume threw for player ${host.handle}", error)
                return
            }
        if (response.ok) {
            host.playbackStarted()
        } else {
            host.cancelPlaybackIntent()
            abandonAudioFocusIfIdle()
        }
        reportBackgroundCommand(host, source, "play", response)
        drainEvents(host)
    }

    private fun prepareNativeArguments(
        method: String,
        arguments: Map<String, Any?>,
        cancellation: AndroidContentPreparationCancellation? = null,
    ): PreparedNativeArguments {
        val nativeArguments = arguments.toMutableMap()
        nativeArguments.remove("playerId")
        if (method == "open") {
            nativeArguments.remove("metadata")
        }
        if (method !in URI_METHODS) {
            return PreparedNativeArguments(nativeArguments, null)
        }
        val rawUri = nativeArguments["uri"] as? String
            ?: return PreparedNativeArguments(nativeArguments, null)
        if (!rawUri.startsWith("content://", ignoreCase = true)) {
            return PreparedNativeArguments(nativeArguments, null)
        }
        val source = detachContentSource(
            Uri.parse(rawUri),
            cancellation ?: AndroidContentPreparationCancellation(),
        )
        nativeArguments["uri"] = source.uri
        return PreparedNativeArguments(nativeArguments, source.fd)
    }

    private fun detachContentSource(
        uri: Uri,
        cancellation: AndroidContentPreparationCancellation,
    ): DetachedContentSource {
        cancellation.throwIfCancelled()
        val resolver = applicationContext.contentResolver
        val asset = resolver.openAssetFileDescriptor(uri, "r")
        if (asset != null) {
            return asset.use { openedAsset ->
                cancellation.throwIfCancelled()
                val offset = max(0L, openedAsset.startOffset)
                val declaredLength = openedAsset.declaredLength.takeIf { it >= 0L }
                val reportedLength = openedAsset.length.takeIf { it > 0L }
                val probe = probeContentDescriptor(openedAsset.parcelFileDescriptor.fileDescriptor)
                when (probe.transport) {
                    AndroidContentTransport.OWNED_DESCRIPTOR -> {
                        val length = resolveSeekableContentLength(
                            uri = uri,
                            offset = offset,
                            declaredLength = declaredLength,
                            reportedLength = reportedLength,
                            endOffset = probe.endOffset,
                        )
                        Log.i(
                            TAG,
                            androidContentSourceEvent(
                                stage = "zero_copy",
                                authority = uri.authority,
                                fields = linkedMapOf(
                                    "offset" to offset,
                                    "length" to length,
                                ),
                            ),
                        )
                        detachAssetFileDescriptor(openedAsset, offset, length)
                    }
                    AndroidContentTransport.CACHE_SPOOL -> spoolContentSource(
                        uri = uri,
                        sourceOffset = offset,
                        expectedLength = declaredLength,
                        fallbackReason = probe.fallbackReason,
                        cancellation = cancellation,
                        openInput = openedAsset::createInputStream,
                    )
                }
            }
        }
        val descriptor = resolver.openFileDescriptor(uri, "r")
            ?: throw FileNotFoundException("Unable to open Android content URI: $uri")
        return descriptor.use { openedDescriptor ->
            cancellation.throwIfCancelled()
            val reportedLength = openedDescriptor.statSize.takeIf { it > 0L }
            val probe = probeContentDescriptor(openedDescriptor.fileDescriptor)
            when (probe.transport) {
                AndroidContentTransport.OWNED_DESCRIPTOR -> {
                    val length = resolveSeekableContentLength(
                        uri = uri,
                        offset = 0L,
                        declaredLength = null,
                        reportedLength = reportedLength,
                        endOffset = probe.endOffset,
                    )
                    Log.i(
                        TAG,
                        androidContentSourceEvent(
                            stage = "zero_copy",
                            authority = uri.authority,
                            fields = linkedMapOf(
                                "offset" to 0L,
                                "length" to length,
                            ),
                        ),
                    )
                    val fd = openedDescriptor.detachFd()
                    detachedContentSource(fd, 0L, length)
                }
                AndroidContentTransport.CACHE_SPOOL -> spoolContentSource(
                    uri = uri,
                    sourceOffset = 0L,
                    expectedLength = null,
                    fallbackReason = probe.fallbackReason,
                    cancellation = cancellation,
                    openInput = { ParcelFileDescriptor.AutoCloseInputStream(openedDescriptor) },
                )
            }
        }
    }

    private fun detachAssetFileDescriptor(
        asset: AssetFileDescriptor,
        offset: Long,
        length: Long?,
    ): DetachedContentSource {
        val fd = asset.parcelFileDescriptor.detachFd()
        return detachedContentSource(fd, offset, length)
    }

    private fun probeContentDescriptor(fileDescriptor: FileDescriptor): ContentDescriptorProbe {
        val stat = try {
            Os.fstat(fileDescriptor)
        } catch (error: ErrnoException) {
            return ContentDescriptorProbe(
                transport = AndroidContentTransport.CACHE_SPOOL,
                endOffset = null,
                fallbackReason = "fstat_errno_${error.errno}",
            )
        }
        val kind = when {
            OsConstants.S_ISREG(stat.st_mode) -> AndroidContentDescriptorKind.REGULAR_FILE
            OsConstants.S_ISFIFO(stat.st_mode) -> AndroidContentDescriptorKind.FIFO
            OsConstants.S_ISSOCK(stat.st_mode) -> AndroidContentDescriptorKind.SOCKET
            OsConstants.S_ISCHR(stat.st_mode) -> AndroidContentDescriptorKind.CHARACTER_DEVICE
            OsConstants.S_ISBLK(stat.st_mode) -> AndroidContentDescriptorKind.BLOCK_DEVICE
            else -> AndroidContentDescriptorKind.OTHER
        }
        val statSize = stat.st_size.takeIf { it >= 0L }
        val transport = androidContentTransport(kind, statSize)
        return ContentDescriptorProbe(
            transport = transport,
            endOffset = statSize.takeIf {
                transport == AndroidContentTransport.OWNED_DESCRIPTOR
            },
            fallbackReason = if (transport == AndroidContentTransport.CACHE_SPOOL) {
                androidContentFallbackReason(kind)
            } else {
                null
            },
        )
    }

    private fun resolveSeekableContentLength(
        uri: Uri,
        offset: Long,
        declaredLength: Long?,
        reportedLength: Long?,
        endOffset: Long?,
    ): Long? {
        if (endOffset != null && endOffset < offset) {
            logEmptyOrInvalidDescriptor(uri, offset, "offset_beyond_descriptor")
            throw EOFException(
                "Android content descriptor offset $offset exceeds its end $endOffset",
            )
        }
        if (declaredLength != null && endOffset != null) {
            if (declaredLength > endOffset - offset) {
                logEmptyOrInvalidDescriptor(uri, offset, "declared_slice_truncated")
                throw EOFException(
                    "Android content descriptor slice is truncated: offset=$offset, " +
                        "length=$declaredLength, end=$endOffset",
                )
            }
        }
        val length = declaredLength
            ?: endOffset?.minus(offset)
            ?: reportedLength
        if (length == 0L) {
            logEmptyOrInvalidDescriptor(uri, offset, "empty_descriptor")
            throw EOFException("Android content descriptor is empty")
        }
        if (length == null) {
            Log.w(
                TAG,
                androidContentSourceEvent(
                    stage = "length_unknown",
                    authority = uri.authority,
                    fields = linkedMapOf(
                        "mode" to "zero_copy",
                        "reason" to "provider_length_unavailable",
                        "offset" to offset,
                    ),
                ),
            )
        }
        return length
    }

    private fun logEmptyOrInvalidDescriptor(uri: Uri, offset: Long, reason: String) {
        Log.e(
            TAG,
            androidContentSourceEvent(
                stage = "failed",
                authority = uri.authority,
                fields = linkedMapOf(
                    "mode" to "zero_copy",
                    "reason" to reason,
                    "offset" to offset,
                ),
            ),
        )
    }

    private fun spoolContentSource(
        uri: Uri,
        sourceOffset: Long,
        expectedLength: Long?,
        fallbackReason: String?,
        cancellation: AndroidContentPreparationCancellation,
        openInput: () -> InputStream,
    ): DetachedContentSource {
        cancellation.throwIfCancelled()
        awaitContentSpoolStartupScavenge(cancellation)
        val startedAt = SystemClock.elapsedRealtime()
        val policy = AndroidContentSpoolPolicy()
        Log.w(
            TAG,
            androidContentSourceEvent(
                stage = "fallback",
                authority = uri.authority,
                fields = linkedMapOf(
                    "mode" to "cache_spool",
                    "reason" to (fallbackReason ?: "non_seekable_descriptor"),
                    "sourceOffset" to sourceOffset,
                    "declaredLength" to expectedLength,
                    "execution" to "background",
                    "maxBytes" to policy.maxBytes,
                    "minFreeBytes" to policy.minFreeBytes,
                ),
            ),
        )
        var cacheFile: File? = null
        var bytesWritten: Long? = null
        try {
            val cacheDirectory = File(applicationContext.cacheDir, ANDROID_CONTENT_SPOOL_DIRECTORY)
            if (!cacheDirectory.isDirectory &&
                !cacheDirectory.mkdirs() &&
                !cacheDirectory.isDirectory
            ) {
                throw IOException("Unable to create Erika Android content spool directory")
            }
            cancellation.throwIfCancelled()
            val outputFile = File.createTempFile(
                ANDROID_CONTENT_SPOOL_PREFIX,
                ANDROID_CONTENT_SPOOL_SUFFIX,
                cacheDirectory,
            )
            cacheFile = outputFile
            cancellation.trackTemporaryFile(outputFile)
            val input = openInput()
            cancellation.register(input)
            try {
                input.use {
                    FileOutputStream(outputFile).use { output ->
                        cancellation.register(output)
                        try {
                            bytesWritten = AndroidContentSpooler.copy(
                                input = input,
                                output = output,
                                expectedLength = expectedLength,
                                policy = policy,
                                availableBytes = { cacheDirectory.usableSpace },
                                cancelled = { cancellation.isCancelled },
                                onProgress = { bytes -> bytesWritten = bytes },
                            )
                            cancellation.throwIfCancelled()
                            output.fd.sync()
                        } finally {
                            cancellation.unregister(output)
                        }
                    }
                }
            } finally {
                cancellation.unregister(input)
            }
            cancellation.throwIfCancelled()
            val length = checkNotNull(bytesWritten)
            val source = detachCachedContentSource(outputFile, length)
            cancellation.releaseTemporaryFile(outputFile)
            try {
                Log.i(
                    TAG,
                    androidContentSourceEvent(
                        stage = "spool_complete",
                        authority = uri.authority,
                        fields = linkedMapOf(
                            "bytes" to length,
                            "elapsedMs" to SystemClock.elapsedRealtime() - startedAt,
                            "cachePathRetained" to false,
                        ),
                    ),
                )
            } catch (error: Throwable) {
                closeDetachedFileDescriptor(source.fd)
                throw error
            }
            return source
        } catch (error: Throwable) {
            val partialFile = cacheFile
            val deleted = partialFile == null || deleteContentSpoolFile(partialFile)
            partialFile?.let(cancellation::releaseTemporaryFile)
            Log.e(
                TAG,
                androidContentSourceEvent(
                    stage = "failed",
                    authority = uri.authority,
                    fields = linkedMapOf(
                        "mode" to "cache_spool",
                        "reason" to androidContentSourceFailureReason(error),
                        "message" to (error.message ?: error.javaClass.simpleName),
                        "bytes" to bytesWritten,
                        "partialCacheDeleted" to deleted,
                        "elapsedMs" to SystemClock.elapsedRealtime() - startedAt,
                    ),
                ),
                error,
            )
            throw error
        }
    }

    private fun detachCachedContentSource(cacheFile: File, length: Long): DetachedContentSource {
        val descriptor = ParcelFileDescriptor.open(cacheFile, ParcelFileDescriptor.MODE_READ_ONLY)
        val fd = try {
            descriptor.detachFd()
        } catch (error: Throwable) {
            descriptor.close()
            throw error
        }
        val unlinked = try {
            cacheFile.delete()
        } catch (error: Throwable) {
            closeDetachedFileDescriptor(fd)
            throw error
        }
        if (!unlinked) {
            closeDetachedFileDescriptor(fd)
            throw IOException(
                "Unable to unlink Erika Android content spool file ${cacheFile.name}",
            )
        }
        // The path is now gone, but the detached descriptor keeps the inode alive until Rust
        // drops its OwnedFileDescriptorSource. No cache path can leak across player lifetimes.
        return detachedContentSource(fd, 0L, length)
    }

    private fun detachedContentSource(
        fd: Int,
        offset: Long,
        length: Long?,
    ): DetachedContentSource = try {
        DetachedContentSource(fd, fdUri(fd, offset, length))
    } catch (error: Throwable) {
        closeDetachedFileDescriptor(fd)
        throw error
    }

    private fun fdUri(fd: Int, offset: Long, length: Long?): String = buildString {
        append("fd://")
        append(fd)
        append("?offset=")
        append(max(0L, offset))
        if (length != null) {
            append("&length=")
            append(length)
        }
    }

    private fun closeDetachedFileDescriptor(fd: Int) {
        runCatching { ParcelFileDescriptor.adoptFd(fd).close() }
            .onFailure { error -> Log.w(TAG, "Unable to close detached content fd $fd", error) }
    }

    private fun handleAudioFocusLoss(mayResume: Boolean) {
        players.values.toList().forEach { host ->
            if (host.playbackPhase == AndroidPlaybackPhase.PAUSED) {
                return@forEach
            }
            val shouldPause = host.handleFocusLoss(mayResume)
            if (shouldPause) {
                runCatching { host.invoke("pause", emptyMap()) }
                    .onSuccess { response ->
                        reportBackgroundCommand(host, "audio focus", "pause", response)
                    }
                    .onFailure { error -> Log.e(TAG, "Audio-focus pause threw", error) }
            }
            drainEvents(host)
        }
        if (!mayResume) {
            abandonAudioFocusIfIdle()
        }
        refreshFrameScheduling()
    }

    private fun handleAudioFocusGain() {
        if (!audioFocus.focusGranted) {
            return
        }
        players.values.toList()
            .filter {
                it.playbackPhase == AndroidPlaybackPhase.PENDING &&
                    (isActivityActive || it.mediaState.allowBackgroundPlayback)
            }
            .forEach { host -> startPendingPlayback(host, "audio focus") }
        refreshFrameScheduling()
    }

    private fun abandonAudioFocusIfIdle() {
        if (players.values.none { it.playbackPhase != AndroidPlaybackPhase.PAUSED }) {
            audioFocus.abandon()
        }
    }

    private fun reportBackgroundCommand(
        host: AndroidPlayerHost,
        source: String,
        method: String,
        response: NativeResponse,
    ) {
        if (!response.ok) {
            Log.e(
                TAG,
                "$source $method failed for player ${host.handle}: " +
                    "status=${response.status} ${response.error.orEmpty()}",
            )
        }
    }

    private fun refreshFrameScheduling() {
        videoViews.values.forEach { view ->
            val phase = view.boundPlayerHost?.playbackPhase
            view.setPlaybackKeepsScreenOn(
                isActivityActive && phase != null && phase != AndroidPlaybackPhase.PAUSED,
            )
        }
        val needsFrame = isActivityActive && players.values.any(AndroidPlayerHost::shouldTick)
        if (!needsFrame) {
            cancelFrameCallback()
            return
        }
        if (!frameScheduled) {
            frameScheduled = true
            choreographer.postFrameCallback(frameCallback)
        }
    }

    private fun performBackgroundPlaybackTick(@Suppress("UNUSED_PARAMETER") timeSeconds: Double) {
        if (isActivityActive) {
            return
        }
        players.values.toList()
            .filter {
                it.mediaState.allowBackgroundPlayback &&
                    it.playbackPhase == AndroidPlaybackPhase.PLAYING
            }
            .forEach { host ->
                runCatching { host.audioOnlyTick() }
                    .onSuccess { response -> reportRenderResponse(host, response) }
                    .onFailure { error -> reportRenderException(host, error) }
            }
        players.values.toList().forEach(::drainEvents)
    }

    private fun cancelFrameCallback() {
        if (!frameScheduled) {
            return
        }
        choreographer.removeFrameCallback(frameCallback)
        frameScheduled = false
    }

    private fun reportRenderResponse(host: AndroidPlayerHost, response: NativeResponse) {
        if (response.ok) {
            host.lastRenderError = null
            return
        }
        val signature = "${response.status}:${response.error.orEmpty()}"
        if (host.lastRenderError != signature) {
            host.lastRenderError = signature
            Log.e(TAG, "renderTick failed for player ${host.handle}: $signature")
            enqueueHostError(
                host,
                "renderTick",
                response.status,
                response.error ?: "renderTick failed",
            )
        }
    }

    private fun reportRenderException(host: AndroidPlayerHost, error: Throwable) {
        val signature = "exception:${error.message.orEmpty()}"
        if (host.lastRenderError != signature) {
            host.lastRenderError = signature
            Log.e(TAG, "renderTick threw for player ${host.handle}", error)
            enqueueHostError(
                host,
                "renderTick",
                -1,
                error.message ?: "renderTick threw",
            )
        }
    }

    private fun enqueueHostError(
        host: AndroidPlayerHost,
        stage: String,
        status: Int,
        error: String,
        details: Map<String, Any?> = emptyMap(),
    ) {
        val event = linkedMapOf<String, Any?>(
            "playerId" to host.handle,
            "kind" to ERROR_EVENT_KIND,
            "state" to ERROR_STATE,
            "status" to status,
            "error" to error,
            "message" to "Android host failure during $stage",
            "hostStage" to stage,
        )
        event.putAll(details)
        enqueuePendingEvent(host, AndroidPendingEvent.Success(event))
        flushPendingEvents(host)
    }

    private fun enqueuePendingEvent(host: AndroidPlayerHost, event: AndroidPendingEvent) {
        val overflow = host.enqueuePendingEvent(event) ?: return
        if (overflow.droppedTotal == 1L ||
            overflow.droppedTotal % EVENT_OVERFLOW_LOG_INTERVAL == 0L
        ) {
            val droppedType = when (val dropped = overflow.dropped) {
                is AndroidPendingEvent.Success ->
                    "success(kind=${(dropped.value["kind"] as? Number)?.toInt()})"
                is AndroidPendingEvent.Error -> "error(code=${dropped.code})"
            }
            Log.w(
                TAG,
                "pendingEventQueueOverflow playerId=${host.handle} " +
                    "policy=drop_oldest capacity=${overflow.capacity} " +
                    "droppedTotal=${overflow.droppedTotal} dropped=$droppedType",
            )
        }
    }

    private fun flushPendingEvents(host: AndroidPlayerHost) {
        val sink = eventSink ?: return
        while (eventSink === sink) {
            val event = host.firstPendingEvent() ?: return
            try {
                when (event) {
                    is AndroidPendingEvent.Success -> sink.success(event.value)
                    is AndroidPendingEvent.Error ->
                        sink.error(event.code, event.message, event.details)
                }
            } catch (error: Throwable) {
                Log.e(
                    TAG,
                    "EventChannel delivery failed for player ${host.handle}; retaining event",
                    error,
                )
                return
            }
            host.removeFirstPendingEvent()
        }
    }

    private fun drainEvents(host: AndroidPlayerHost) {
        var latestPlaybackState: Int? = null
        for (index in 0 until MAX_EVENTS_PER_FRAME) {
            val response = try {
                host.pollEvent()
            } catch (error: Throwable) {
                Log.e(TAG, "pollEvent threw for player ${host.handle}", error)
                break
            } ?: break
            if (!response.ok) {
                if (response.status != NO_EVENT_STATUS) {
                    enqueuePendingEvent(
                        host,
                        AndroidPendingEvent.Error(
                            code = "ERIKA_ERROR",
                            message = response.error ?: "Erika event polling failed",
                            details = mapOf(
                                "playerId" to host.handle,
                                "status" to response.status,
                            ),
                        ),
                    )
                }
                break
            }
            val rawEvent = response.value as? Map<*, *> ?: break
            val event = linkedMapOf<String, Any?>()
            rawEvent.forEach { (key, value) ->
                if (key != null) {
                    event[key.toString()] = value
                }
            }
            event.putIfAbsent("playerId", host.handle)
            if ((event["kind"] as? Number)?.toInt() == ERROR_EVENT_KIND) {
                val status = (event["status"] as? Number)?.toInt() ?: -1
                val error = event["error"] as? String
                    ?: event["message"] as? String
                    ?: "unknown native error"
                Log.e(
                    TAG,
                    "Erika error event: playerId=${host.handle} status=$status error=$error",
                )
            }
            latestPlaybackState = updatedPlaybackState(latestPlaybackState, event)
            host.updateMediaState(event)
            enqueuePendingEvent(host, AndroidPendingEvent.Success(event))
        }
        latestPlaybackState?.let { state ->
            observeNativePlaybackState(host, state)
        }
        if (activeMediaPlayerId == host.handle) {
            mediaSession.update(host.mediaState)
        }
        flushPendingEvents(host)
    }

    private fun setMediaMetadata(arguments: Map<String, Any?>, result: MethodChannel.Result) {
        val host = player(arguments)
        host.setMediaMetadata(androidMediaMetadata(arguments))
        if (activeMediaPlayerId == host.handle) {
            mediaSession.update(host.mediaState)
        }
        result.success(null)
    }

    private fun setSystemMediaNavigation(
        arguments: Map<String, Any?>,
        result: MethodChannel.Result,
    ) {
        val host = player(arguments)
        host.setSystemMediaNavigation(arguments)
        if (activeMediaPlayerId == host.handle) {
            mediaSession.update(host.mediaState)
        }
        result.success(null)
    }

    private fun emitSystemMediaNavigation(playerId: Long, navigation: String) {
        val host = players[playerId] ?: return
        val event = systemMediaNavigationEvent(host.mediaState, navigation) ?: return
        enqueuePendingEvent(host, AndroidPendingEvent.Success(event))
        flushPendingEvents(host)
    }

    private fun performSystemMediaCommand(
        playerId: Long,
        method: String,
        arguments: Map<String, Any?> = emptyMap(),
    ) {
        val host = players[playerId] ?: return
        if (method == "play") {
            if (!host.mediaState.canPlay(isActivityActive)) {
                return
            }
            host.requestPlayback()
            val granted = runCatching { audioFocus.request() }.getOrNull()
            if (granted != AudioFocusGrant.GRANTED) {
                return
            }
        } else if (method in PLAYBACK_INTENT_CANCEL_METHODS) {
            host.cancelPlaybackIntent()
        }
        runCatching { host.invoke(method, arguments) }
            .onSuccess { response ->
                if (response.ok && method == "play") {
                    host.playbackStarted()
                    activeMediaPlayerId = host.handle
                }
                drainEvents(host)
                if (activeMediaPlayerId == host.handle) {
                    mediaSession.update(host.mediaState)
                }
                refreshFrameScheduling()
            }
            .onFailure { error -> Log.e(TAG, "System media $method threw", error) }
    }

    private fun observeNativePlaybackState(host: AndroidPlayerHost, state: Int) {
        when (state) {
            PLAYING_STATE -> {
                if (isActivityActive &&
                    audioFocus.focusGranted &&
                    host.playbackPhase == AndroidPlaybackPhase.PENDING
                ) {
                    host.playbackStarted()
                }
            }
            PAUSED_STATE -> {
                if (host.playbackPhase == AndroidPlaybackPhase.PLAYING) {
                    host.cancelPlaybackIntent()
                    abandonAudioFocusIfIdle()
                }
            }
            STOPPED_STATE,
            CLOSED_STATE,
            ERROR_STATE -> {
                host.cancelPlaybackIntent()
                abandonAudioFocusIfIdle()
            }
        }
        refreshFrameScheduling()
    }

    private fun complete(result: MethodChannel.Result, response: NativeResponse) {
        if (response.ok) {
            result.success(response.value)
        } else {
            result.error(
                "ERIKA_ERROR",
                response.error ?: "Erika native call failed with status ${response.status}",
                mapOf("status" to response.status),
            )
        }
    }

    private fun player(arguments: Map<String, Any?>): AndroidPlayerHost {
        val playerId = arguments.requiredLong("playerId")
        return players[playerId]
            ?: throw IllegalStateException("Erika Android player $playerId was not found")
    }

    private fun arguments(call: MethodCall): Map<String, Any?> {
        val raw = call.arguments as? Map<*, *> ?: return emptyMap()
        return buildMap {
            raw.forEach { (key, value) ->
                if (key != null) {
                    put(key.toString(), value)
                }
            }
        }
    }

    private fun newContentPreparationExecutor(): ExecutorService =
        Executors.newFixedThreadPool(CONTENT_PREPARATION_THREADS) { runnable ->
            Thread(
                runnable,
                "erika-content-${CONTENT_PREPARATION_THREAD_IDS.getAndIncrement()}",
            ).apply {
                isDaemon = true
            }
        }

    private fun scheduleContentSpoolStartupScavenge(): Future<*>? = try {
        contentPreparationExecutor.submit {
            val startedAt = SystemClock.elapsedRealtime()
            try {
                // cacheDir access and directory enumeration both stay off the platform thread.
                val directory = File(applicationContext.cacheDir, ANDROID_CONTENT_SPOOL_DIRECTORY)
                val stats = scavengeAndroidContentSpoolDirectory(directory)
                val event = androidContentSourceEvent(
                    stage = "startup_scavenge",
                    authority = null,
                    fields = linkedMapOf(
                        "mode" to "cache_spool",
                        "execution" to "background",
                        "files" to stats.files,
                        "bytes" to stats.bytes,
                        "deleteFailures" to stats.deleteFailures,
                        "elapsedMs" to SystemClock.elapsedRealtime() - startedAt,
                    ),
                )
                if (stats.deleteFailures == 0) {
                    Log.i(TAG, event)
                } else {
                    Log.w(TAG, event)
                }
            } catch (error: Throwable) {
                Log.e(
                    TAG,
                    androidContentSourceEvent(
                        stage = "startup_scavenge",
                        authority = null,
                        fields = linkedMapOf(
                            "mode" to "cache_spool",
                            "execution" to "background",
                            "files" to 0,
                            "bytes" to 0L,
                            "deleteFailures" to 0,
                            "reason" to "scan_failed",
                            "message" to (error.message ?: error.javaClass.simpleName),
                            "elapsedMs" to SystemClock.elapsedRealtime() - startedAt,
                        ),
                    ),
                    error,
                )
            }
        }
    } catch (error: RejectedExecutionException) {
        Log.e(
            TAG,
            androidContentSourceEvent(
                stage = "startup_scavenge",
                authority = null,
                fields = linkedMapOf(
                    "mode" to "cache_spool",
                    "execution" to "not_started",
                    "files" to 0,
                    "bytes" to 0L,
                    "deleteFailures" to 0,
                    "reason" to "executor_unavailable",
                ),
            ),
            error,
        )
        null
    }

    private fun awaitContentSpoolStartupScavenge(
        cancellation: AndroidContentPreparationCancellation,
    ) {
        val future = contentSpoolScavengeFuture ?: return
        try {
            future.get()
        } catch (error: InterruptedException) {
            Thread.currentThread().interrupt()
            throw AndroidContentPreparationCancelledException(
                "Android content preparation was interrupted before startup cache cleanup",
            )
        } catch (error: CancellationException) {
            throw AndroidContentPreparationCancelledException(
                "Android content startup cache cleanup was cancelled",
            )
        } catch (error: Throwable) {
            throw IOException("Android content startup cache cleanup failed", error)
        }
        cancellation.throwIfCancelled()
    }

    private class PendingContentCommand(
        val host: AndroidPlayerHost,
        val method: String,
        val authority: String?,
        val result: MethodChannel.Result,
        val cancellation: AndroidContentPreparationCancellation,
    ) {
        lateinit var token: AndroidContentPreparationToken
        private var completed = false

        fun claimCompletion(): Boolean {
            if (completed) {
                return false
            }
            completed = true
            return true
        }
    }

    private data class PreparedNativeArguments(
        val arguments: Map<String, Any?>,
        val detachedFd: Int?,
    )

    private data class DetachedContentSource(
        val fd: Int,
        val uri: String,
    )

    private data class ContentDescriptorProbe(
        val transport: AndroidContentTransport,
        val endOffset: Long?,
        val fallbackReason: String?,
    )

    companion object {
        private const val TAG = "ErikaFlutterPlugin"
        private const val PLAYER_CHANNEL = "erika_flutter/player"
        private const val EVENT_CHANNEL = "erika_flutter/events"
        private const val VIDEO_VIEW_TYPE = "erika_flutter/video_view"
        private const val HDR_VIDEO_VIEW_TYPE = "erika_flutter/hdr_video_view"
        private const val MAX_EVENTS_PER_FRAME = 256
        private const val EVENT_OVERFLOW_LOG_INTERVAL = 256L
        private const val NO_EVENT_STATUS = 5
        private const val ERROR_EVENT_KIND = 9
        private const val PLAYING_STATE = 3
        private const val PAUSED_STATE = 4
        private const val STOPPED_STATE = 5
        private const val CLOSED_STATE = 6
        private const val ERROR_STATE = 7
        private const val NO_OWNED_FD = -1
        private const val CONTENT_PREPARATION_THREADS = 2
        private val CONTENT_PREPARATION_THREAD_IDS = AtomicInteger(1)

        private val URI_METHODS = setOf(
            "open",
            "addExternalSubtitle",
            "loadDanmakuFile",
            "addDanmakuTrackFile",
        )

        private val PLAYBACK_INTENT_CANCEL_METHODS = setOf(
            "open",
            "pause",
            "stop",
            "close",
        )

        private val CONTENT_PREPARATION_INVALIDATION_METHODS = setOf(
            "open",
            "stop",
            "close",
        )

        private val RENDER_REQUEST_METHODS = setOf(
            "open",
            "stop",
            "close",
            "seek",
            "setUpscaler",
            "setSubtitleScale",
            "setSubtitleStyle",
            "addExternalSubtitle",
            "removeSubtitleTrack",
            "loadDanmakuFile",
            "loadDanmakuJson",
            "addDanmakuTrackFile",
            "addDanmakuTrackJson",
            "removeDanmakuTrack",
            "setDanmakuTrackEnabled",
            "setDanmakuTrackOffset",
            "setDanmakuGlobalOffset",
            "clearDanmaku",
            "setDanmakuEnabled",
            "setDanmakuConfig",
            "setDebugHudEnabled",
            "selectAudioTrack",
            "selectSubtitleTrack",
        )

        private val NATIVE_METHODS = setOf(
            "open",
            "play",
            "pause",
            "stop",
            "close",
            "seek",
            "setPlaybackRate",
            "setVolume",
            "setUpscaler",
            "setSubtitleScale",
            "setSubtitleStyle",
            "getUpscalerStatus",
            "getOutputStatus",
            "getPresenterStats",
            "setDebugHudEnabled",
            "addExternalSubtitle",
            "removeSubtitleTrack",
            "loadDanmakuFile",
            "loadDanmakuJson",
            "addDanmakuTrackFile",
            "addDanmakuTrackJson",
            "removeDanmakuTrack",
            "setDanmakuTrackEnabled",
            "setDanmakuTrackOffset",
            "setDanmakuGlobalOffset",
            "danmakuTracks",
            "clearDanmaku",
            "setDanmakuEnabled",
            "setDanmakuConfig",
            "selectAudioTrack",
            "selectSubtitleTrack",
            "tracks",
        )
    }
}

private fun Map<String, Any?>.number(key: String): Number? = this[key] as? Number

private fun Map<String, Any?>.int(key: String): Int? = number(key)?.toInt()

private fun Map<String, Any?>.requiredInt(key: String): Int =
    int(key) ?: throw IllegalArgumentException("Missing integer argument '$key'")

private fun Map<String, Any?>.requiredLong(key: String): Long =
    number(key)?.toLong() ?: throw IllegalArgumentException("Missing integer argument '$key'")
