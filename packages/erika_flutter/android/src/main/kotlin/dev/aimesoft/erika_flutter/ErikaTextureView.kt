package dev.aimesoft.erika_flutter

import android.content.Context
import android.graphics.SurfaceTexture
import android.graphics.PixelFormat
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.view.Display
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.TextureView
import android.view.View
import io.flutter.plugin.common.StandardMessageCodec
import io.flutter.plugin.platform.PlatformView
import io.flutter.plugin.platform.PlatformViewFactory
import java.util.function.Consumer
import kotlin.math.max

internal class ErikaAndroidVideoViewFactory(
    private val plugin: ErikaFlutterPlugin,
    private val useHdrSurface: Boolean = false,
) : PlatformViewFactory(StandardMessageCodec.INSTANCE) {
    override fun create(context: Context, viewId: Int, args: Any?): PlatformView {
        @Suppress("UNCHECKED_CAST")
        val creationParams = args as? Map<String, Any?> ?: emptyMap()
        return ErikaAndroidVideoView(
            context,
            viewId,
            creationParams,
            plugin,
            useHdrSurface,
        )
    }
}

internal class ErikaAndroidVideoView(
    context: Context,
    val viewId: Int,
    creationParams: Map<String, Any?>,
    private val plugin: ErikaFlutterPlugin,
    private val useHdrSurface: Boolean,
) : PlatformView,
    TextureView.SurfaceTextureListener,
    SurfaceHolder.Callback2 {
    private val textureView = if (useHdrSurface) null else TextureView(context)
    private val surfaceView = if (useHdrSurface) SurfaceView(context) else null
    private val nativeView: View = surfaceView ?: requireNotNull(textureView)
    private val rawRequestedHdrHeadroom =
        (creationParams["requestedHdrHeadroom"] as? Number)?.toFloat()
    private val requestedHdrHeadroom = androidDesiredHdrHeadroom(rawRequestedHdrHeadroom)
    private val hybridComposition = creationParams["composition"] == "hybrid"
    private val mainHandler = Handler(Looper.getMainLooper())
    private var outputSurface: Surface? = null
    private var ownsOutputSurface = false
    private var surfacePixelWidth = 0
    private var surfacePixelHeight = 0
    private var boundHost: AndroidPlayerHost? = null
    private var pendingBind: PendingViewBind? = null
    private var nativeDetachRetryPending = false
    private var unbindRequested = false
    private var disposeRequested = false
    private var disposed = false
    private val surfaceRecoveryTokens = AndroidSurfaceRecoveryTokenSource()
    private var surfaceRecoveryRunnable: Runnable? = null
    private var observedHdrDisplay: Display? = null
    private var hdrRatioListenerRegistered = false
    private var attachedDisplayId: Int? = null
    private var attachedDisplayHdrSupported: Boolean? = null
    private var lastPublishedHdrHeadroom: AndroidHdrHeadroomState? = null
    private val hdrRatioListener = Consumer<Display> {
        mainHandler.post { refreshHdrHeadroomObservation() }
    }
    private val attachStateListener = object : View.OnAttachStateChangeListener {
        override fun onViewAttachedToWindow(view: View) {
            mainHandler.post {
                if (disposed || disposeRequested) {
                    return@post
                }
                val host = boundHost
                if (host != null && !host.surfaceAttached) {
                    val attempt = attachIfReady(host)
                    handleImmediateAttempt(host, attempt)
                }
                refreshHdrHeadroomObservation()
            }
        }

        override fun onViewDetachedFromWindow(view: View) {
            stopHdrHeadroomObservation(publishUnknown = true)
        }
    }

    internal val boundPlayerHost: AndroidPlayerHost?
        get() = boundHost

    internal val isExtendedLinearSurface: Boolean
        get() = useHdrSurface

    init {
        if (rawRequestedHdrHeadroom != null && rawRequestedHdrHeadroom != requestedHdrHeadroom) {
            Log.w(
                TAG,
                "invalid requestedHdrHeadroom=$rawRequestedHdrHeadroom for viewId=$viewId; " +
                    "using 0 (system auto), expected 0 or [1, 10000]",
            )
        }
        textureView?.apply {
            isOpaque = true
            surfaceTextureListener = this@ErikaAndroidVideoView
        }
        surfaceView?.apply {
            holder.setFormat(PixelFormat.RGBA_F16)
            holder.addCallback(this@ErikaAndroidVideoView)
            if (Build.VERSION.SDK_INT >= 35) {
                runCatching { setDesiredHdrHeadroom(requestedHdrHeadroom) }
                    .onFailure { error ->
                        Log.w(
                            TAG,
                            "setDesiredHdrHeadroom failed viewId=$viewId requested=$requestedHdrHeadroom",
                            error,
                        )
                    }
            }
        }
        nativeView.addOnAttachStateChangeListener(attachStateListener)
        nativeView.contentDescription = creationParams["debugLabel"] as? String
        plugin.registerVideoView(this)
    }

    override fun getView(): View = nativeView

    override fun dispose() {
        if (disposed) {
            return
        }
        disposeRequested = true
        val host = boundHost
        if (host == null) {
            cancelSurfaceRecovery()
            finishDispose()
            return
        }
        unbind(host)
    }

    private fun finishDispose() {
        if (disposed) {
            return
        }
        stopHdrHeadroomObservation(publishUnknown = false)
        cancelSurfaceRecovery()
        disposed = true
        pendingBind = null
        nativeDetachRetryPending = false
        unbindRequested = false
        releaseSurface()
        textureView?.surfaceTextureListener = null
        surfaceView?.holder?.removeCallback(this)
        nativeView.removeOnAttachStateChangeListener(attachStateListener)
        plugin.unregisterVideoView(this)
    }

    fun bind(host: AndroidPlayerHost): NativeResponse {
        if (disposed || disposeRequested) {
            return NativeResponse(false, -1, "Android video view $viewId is disposed", null)
        }
        if (boundHost !== host) {
            boundHost?.let { currentHost ->
                clearPendingBind()
                val response = unbind(currentHost)
                if (!response.ok) {
                    queuePendingBind(host, this)
                    return response
                }
            }
            host.attachedView?.takeIf { it !== this }?.let { previousView ->
                previousView.clearPendingBind()
                val response = previousView.unbind(host)
                if (!response.ok) {
                    previousView.queuePendingBind(host, this)
                    return response
                }
            }
            boundHost = host
            host.attachedView = this
            lastPublishedHdrHeadroom = null
        }
        unbindRequested = false
        cancelSurfaceRecovery()
        val attempt = attachIfReady(host)
        handleImmediateAttempt(host, attempt)
        refreshHdrHeadroomObservation()
        plugin.onPlayerRenderStateChanged()
        return attempt.response
    }

    fun unbind(expectedHost: AndroidPlayerHost? = null): NativeResponse {
        val host = boundHost
        if (host == null) {
            cancelSurfaceRecovery()
            if (disposeRequested) {
                finishDispose()
            }
            return NativeResponse.success()
        }
        if (expectedHost != null && host !== expectedHost) {
            return NativeResponse.success()
        }
        unbindRequested = true
        stopHdrHeadroomObservation(publishUnknown = true)
        cancelSurfaceRecovery()
        val response = detachNativeSurface(host)
        plugin.reportSurfaceResponse(host, "detachSurface", response)
        if (!response.ok) {
            startSurfaceRecovery(host, "detachSurface", response)
            return response
        }
        completeUnbind(host)
        return response
    }

    fun suspendSurface(): NativeResponse {
        val host = boundHost ?: return NativeResponse.success()
        if (unbindRequested || disposeRequested) {
            return unbind(host)
        }
        unbindRequested = false
        stopHdrHeadroomObservation(publishUnknown = true)
        cancelSurfaceRecovery()
        val response = detachNativeSurface(host)
        plugin.reportSurfaceResponse(host, "detachSurface", response)
        if (!response.ok) {
            startSurfaceRecovery(host, "detachSurface", response)
        }
        plugin.onPlayerRenderStateChanged()
        return response
    }

    fun resumeSurface(): NativeResponse {
        val host = boundHost ?: return NativeResponse.success()
        if (unbindRequested || disposeRequested) {
            return unbind(host)
        }
        unbindRequested = false
        cancelSurfaceRecovery()
        val attempt = attachIfReady(host)
        handleImmediateAttempt(host, attempt)
        refreshHdrHeadroomObservation()
        plugin.onPlayerRenderStateChanged()
        return attempt.response
    }

    fun setFlutterManagedVisibility(visible: Boolean, debugLabel: String?) {
        nativeView.visibility = if (visible) View.VISIBLE else View.INVISIBLE
        if (debugLabel != null) {
            nativeView.contentDescription = debugLabel
        }
    }

    fun setPlaybackKeepsScreenOn(enabled: Boolean) {
        nativeView.keepScreenOn = enabled
    }

    fun pixelWidth(): Int = surfacePixelWidth.takeIf { it > 0 } ?: nativeView.width

    fun pixelHeight(): Int = surfacePixelHeight.takeIf { it > 0 } ?: nativeView.height

    internal fun onPlayerDestroyed(host: AndroidPlayerHost) {
        if (pendingBind?.host === host) {
            pendingBind = null
        }
        if (boundHost !== host) {
            return
        }
        val deferredBind = takePendingBind()
        cancelSurfaceRecovery()
        nativeDetachRetryPending = false
        unbindRequested = false
        boundHost = null
        if (disposeRequested) {
            finishDispose()
        }
        resumePendingBind(deferredBind)
        plugin.onPlayerRenderStateChanged()
    }

    override fun onSurfaceTextureAvailable(surfaceTexture: SurfaceTexture, width: Int, height: Int) {
        surfaceTexture.setDefaultBufferSize(max(1, width), max(1, height))
        onNativeSurfaceAvailable(Surface(surfaceTexture), width, height, ownsSurface = true)
    }

    override fun onSurfaceTextureSizeChanged(surfaceTexture: SurfaceTexture, width: Int, height: Int) {
        surfaceTexture.setDefaultBufferSize(max(1, width), max(1, height))
        onNativeSurfaceSizeChanged(width, height)
    }

    override fun onSurfaceTextureDestroyed(surfaceTexture: SurfaceTexture): Boolean =
        onNativeSurfaceDestroyed()

    override fun onSurfaceTextureUpdated(surfaceTexture: SurfaceTexture) = Unit

    override fun surfaceCreated(holder: SurfaceHolder) {
        onNativeSurfaceAvailable(
            holder.surface,
            nativeView.width,
            nativeView.height,
            ownsSurface = false,
        )
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (outputSurface == null) {
            onNativeSurfaceAvailable(holder.surface, width, height, ownsSurface = false)
        } else {
            onNativeSurfaceSizeChanged(width, height)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        onNativeSurfaceDestroyed()
    }

    override fun surfaceRedrawNeeded(holder: SurfaceHolder) {
        boundHost?.requestRender()
        plugin.onPlayerRenderStateChanged()
    }

    private fun onNativeSurfaceAvailable(
        surface: Surface,
        width: Int,
        height: Int,
        ownsSurface: Boolean,
    ) {
        cancelSurfaceRecovery()
        surfacePixelWidth = max(1, width)
        surfacePixelHeight = max(1, height)
        var detachResponse = NativeResponse.success()
        val host = boundHost
        if (host != null && (host.surfaceAttached || nativeDetachRetryPending)) {
            detachResponse = detachNativeSurface(host)
            plugin.reportSurfaceResponse(host, "detachSurface", detachResponse)
        }
        releaseSurface()
        outputSurface = surface
        ownsOutputSurface = ownsSurface
        if (!detachResponse.ok) {
            if (host != null) {
                startSurfaceRecovery(host, "detachSurface", detachResponse)
            }
            plugin.onPlayerRenderStateChanged()
            return
        }
        if (host != null && (unbindRequested || disposeRequested)) {
            completeUnbind(host)
            return
        }
        host?.let { currentHost ->
            val attempt = attachIfReady(currentHost)
            handleImmediateAttempt(currentHost, attempt)
        }
        refreshHdrHeadroomObservation()
        plugin.onPlayerRenderStateChanged()
    }

    private fun onNativeSurfaceSizeChanged(width: Int, height: Int) {
        cancelSurfaceRecovery()
        surfacePixelWidth = max(1, width)
        surfacePixelHeight = max(1, height)
        val host = boundHost ?: return
        if (unbindRequested || disposeRequested) {
            val response = detachNativeSurface(host)
            plugin.reportSurfaceResponse(host, "detachSurface", response)
            if (response.ok) {
                completeUnbind(host)
            } else {
                startSurfaceRecovery(host, "detachSurface", response)
            }
            return
        }
        val metrics = surfaceMetrics(width, height)
        if (nativeDetachRetryPending) {
            val attempt = attachIfReady(host)
            handleImmediateAttempt(host, attempt)
        } else if (host.surfaceAttached) {
            plugin.reportSurfaceResponse(
                host,
                "resizeSurface",
                host.resizeSurface(metrics.width, metrics.height, metrics.scale),
            )
        } else {
            val attempt = attachIfReady(host)
            handleImmediateAttempt(host, attempt)
        }
        refreshHdrHeadroomObservation()
        plugin.onPlayerRenderStateChanged()
    }

    private fun onNativeSurfaceDestroyed(): Boolean {
        stopHdrHeadroomObservation(publishUnknown = true)
        cancelSurfaceRecovery()
        val host = boundHost
        val response = host?.let { host ->
            val response = detachNativeSurface(host)
            plugin.reportSurfaceResponse(host, "detachSurface", response)
            response
        } ?: NativeResponse.success()
        val decision = androidSurfaceDestroyDecision(response.ok)
        nativeDetachRetryPending = decision.retryNativeDetach
        releaseSurface()
        surfacePixelWidth = 0
        surfacePixelHeight = 0
        if (host != null) {
            if (response.ok && (unbindRequested || disposeRequested)) {
                completeUnbind(host)
            } else if (decision.retryNativeDetach) {
                startSurfaceRecovery(host, "detachSurface", response)
            }
        }
        plugin.onPlayerRenderStateChanged()
        return decision.releaseSurfaceTexture
    }

    private fun attachIfReady(host: AndroidPlayerHost): SurfaceAttempt {
        if (nativeDetachRetryPending) {
            val response = detachNativeSurface(host)
            if (!response.ok) {
                return SurfaceAttempt("detachSurface", response)
            }
        }
        if (!plugin.isActivityActive || host.surfaceAttached) {
            return SurfaceAttempt("attachSurface", NativeResponse.success())
        }
        val surface = outputSurface
            ?: return SurfaceAttempt("attachSurface", NativeResponse.success())
        if (!surface.isValid) {
            return SurfaceAttempt("attachSurface", NativeResponse.success())
        }
        val display = nativeView.display
        if (useHdrSurface && display == null) {
            Log.i(
                TAG,
                "surfaceOutputCapability pending playerId=${host.handle} viewId=$viewId " +
                    "reason=display_not_attached_yet",
            )
            return SurfaceAttempt("attachSurface", NativeResponse.success())
        }
        val metrics = surfaceMetrics(pixelWidth(), pixelHeight())
        val displayHdrSupported = display?.let(::displaySupportsHdr) == true
        val directComposition = useHdrSurface && hybridComposition
        val outputCapability = androidOutputCapabilityDecision(
            extendedLinearRequested = useHdrSurface,
            sdkInt = Build.VERSION.SDK_INT,
            displayHdrSupported = displayHdrSupported,
            directComposition = directComposition,
        )
        Log.i(
            TAG,
            "surfaceOutputCapability playerId=${host.handle} viewId=$viewId " +
                "requestedExtendedLinear=$useHdrSurface " +
                "eligible=${outputCapability.extendedLinearEligible} " +
                "directComposition=$directComposition sdk=${Build.VERSION.SDK_INT} " +
                "requestedHeadroom=$requestedHdrHeadroom " +
                "fallbackReason=${androidOutputFallbackReasonLabel(outputCapability.fallbackReason)}" +
                "(${outputCapability.fallbackReason})",
        )
        val response = try {
                host.attachSurface(
                    surface,
                    metrics.width,
                    metrics.height,
                    metrics.scale,
                    outputCapability.extendedLinearEligible,
                    directComposition,
                    requestedHdrHeadroom,
                    outputCapability.fallbackReason,
                )
            } catch (error: Throwable) {
                surfaceOperationException(host, "attachSurface", error)
            }
        if (response.ok) {
            attachedDisplayId = display?.displayId
            attachedDisplayHdrSupported = displayHdrSupported
        }
        return SurfaceAttempt("attachSurface", response)
    }

    private fun displaySupportsHdr(display: Display): Boolean {
        // Display.isHdr is derived from getHdrCapabilities(), so it respects
        // user-disabled HDR output types. Display.Mode.supportedHdrTypes is
        // only the raw hardware list and can incorrectly keep FP16 output
        // eligible after the user disables every HDR type.
        return runCatching { display.isHdr }
            .onFailure { error ->
                Log.w(
                    TAG,
                    "display HDR capability query failed viewId=$viewId " +
                        "displayId=${display.displayId}",
                    error,
                )
            }
            .getOrDefault(false)
    }

    private fun refreshHdrHeadroomObservation() {
        if (
            !useHdrSurface ||
            Build.VERSION.SDK_INT < 34 ||
            disposed ||
            unbindRequested ||
            disposeRequested ||
            !plugin.isActivityActive ||
            !nativeView.isAttachedToWindow
        ) {
            stopHdrHeadroomObservation(publishUnknown = true)
            return
        }
        val host = boundHost ?: run {
            stopHdrHeadroomObservation(publishUnknown = false)
            return
        }
        val display = nativeView.display ?: run {
            stopHdrHeadroomObservation(publishUnknown = true)
            return
        }
        val displayHdrSupported = displaySupportsHdr(display)
        val displayCapabilityChanged = host.surfaceAttached &&
            (attachedDisplayId != display.displayId ||
                attachedDisplayHdrSupported != displayHdrSupported)
        if (displayCapabilityChanged) {
            Log.i(
                TAG,
                "surfaceDisplayChanged playerId=${host.handle} viewId=$viewId " +
                    "oldDisplayId=$attachedDisplayId newDisplayId=${display.displayId} " +
                    "oldHdr=$attachedDisplayHdrSupported newHdr=$displayHdrSupported " +
                    "action=detach_and_reattach",
            )
            stopHdrHeadroomObservation(publishUnknown = false)
            val detachResponse = detachNativeSurface(host)
            plugin.reportSurfaceResponse(host, "detachSurface", detachResponse)
            if (!detachResponse.ok) {
                startSurfaceRecovery(host, "detachSurface", detachResponse)
                return
            }
            val attachAttempt = attachIfReady(host)
            handleImmediateAttempt(host, attachAttempt)
            if (!attachAttempt.response.ok || !host.surfaceAttached) {
                return
            }
        }

        if (observedHdrDisplay !== display) {
            stopHdrHeadroomObservation(publishUnknown = false)
            observedHdrDisplay = display
        }
        val ratioAvailable = runCatching { display.isHdrSdrRatioAvailable }
            .onFailure { error ->
                Log.w(
                    TAG,
                    "isHdrSdrRatioAvailable failed playerId=${host.handle} " +
                        "viewId=$viewId displayId=${display.displayId}",
                    error,
                )
            }
            .getOrDefault(false)
        if (ratioAvailable && !hdrRatioListenerRegistered) {
            runCatching {
                display.registerHdrSdrRatioChangedListener(
                    nativeView.context.mainExecutor,
                    hdrRatioListener,
                )
            }.onSuccess {
                hdrRatioListenerRegistered = true
            }.onFailure { error ->
                Log.w(
                    TAG,
                    "registerHdrSdrRatioChangedListener failed playerId=${host.handle} " +
                        "viewId=$viewId displayId=${display.displayId}",
                    error,
                )
            }
        } else if (!ratioAvailable && hdrRatioListenerRegistered) {
            stopHdrHeadroomObservation(publishUnknown = false)
            observedHdrDisplay = display
        }
        publishHdrHeadroom(host, display, ratioAvailable)
    }

    private fun publishHdrHeadroom(
        host: AndroidPlayerHost,
        display: Display,
        ratioAvailable: Boolean,
    ) {
        val ratio = if (ratioAvailable) {
            runCatching { display.hdrSdrRatio }.getOrElse { error ->
                Log.w(
                    TAG,
                    "getHdrSdrRatio failed playerId=${host.handle} viewId=$viewId " +
                        "displayId=${display.displayId}",
                    error,
                )
                Float.NaN
            }
        } else {
            Float.NaN
        }
        val state = androidHdrHeadroomState(ratioAvailable, ratio)
        if (lastPublishedHdrHeadroom == state) {
            return
        }
        val response = try {
            host.setOutputHeadroom(state.headroom, state.known)
        } catch (error: Throwable) {
            surfaceOperationException(host, "setOutputHeadroom", error)
        }
        if (!response.ok) {
            plugin.reportSurfaceResponse(host, "setOutputHeadroom", response)
        } else {
            lastPublishedHdrHeadroom = state
        }
        Log.i(
            TAG,
            "surfaceHeadroom playerId=${host.handle} viewId=$viewId " +
                "displayId=${display.displayId} ratio=${state.headroom} " +
                "known=${state.known} requested=$requestedHdrHeadroom " +
                "status=${response.status} error=${response.error.orEmpty()}",
        )
    }

    private fun stopHdrHeadroomObservation(publishUnknown: Boolean) {
        val display = observedHdrDisplay
        if (Build.VERSION.SDK_INT >= 34 && hdrRatioListenerRegistered && display != null) {
            runCatching { display.unregisterHdrSdrRatioChangedListener(hdrRatioListener) }
                .onFailure { error ->
                    Log.w(
                        TAG,
                        "unregisterHdrSdrRatioChangedListener failed viewId=$viewId " +
                            "displayId=${display.displayId}",
                        error,
                    )
                }
        }
        hdrRatioListenerRegistered = false
        observedHdrDisplay = null
        if (publishUnknown && useHdrSurface) {
            boundHost?.let { host ->
                val unknown = AndroidHdrHeadroomState(1f, false)
                if (lastPublishedHdrHeadroom == unknown) {
                    return@let
                }
                runCatching { host.setOutputHeadroom(unknown.headroom, unknown.known) }
                    .onSuccess { response ->
                        if (response.ok) {
                            lastPublishedHdrHeadroom = unknown
                        } else {
                            plugin.reportSurfaceResponse(host, "setOutputHeadroom", response)
                        }
                    }
                    .onFailure { error ->
                        Log.w(
                            TAG,
                            "setOutputHeadroom unknown failed playerId=${host.handle} " +
                                "viewId=$viewId",
                            error,
                        )
                    }
            }
        }
    }

    private fun detachNativeSurface(host: AndroidPlayerHost): NativeResponse {
        val response = try {
            host.detachSurface()
        } catch (error: Throwable) {
            surfaceOperationException(host, "detachSurface", error)
        }
        nativeDetachRetryPending = !response.ok && host.surfaceAttached
        if (response.ok) {
            attachedDisplayId = null
            attachedDisplayHdrSupported = null
        }
        return response
    }

    private fun surfaceOperationException(
        host: AndroidPlayerHost,
        operation: String,
        error: Throwable,
    ): NativeResponse {
        val message = error.message ?: "$operation threw without an error message"
        Log.e(
            TAG,
            "surfaceOperationException playerId=${host.handle} viewId=$viewId " +
                "operation=$operation error=$message",
            error,
        )
        return NativeResponse(false, -1, "$operation threw: $message", null)
    }

    private fun handleImmediateAttempt(host: AndroidPlayerHost, attempt: SurfaceAttempt) {
        plugin.reportSurfaceResponse(host, attempt.operation, attempt.response)
        if (!attempt.response.ok) {
            startSurfaceRecovery(host, attempt.operation, attempt.response)
        }
    }

    private fun startSurfaceRecovery(
        host: AndroidPlayerHost,
        operation: String,
        response: NativeResponse,
    ) {
        if (disposed || boundHost !== host) {
            return
        }
        scheduleSurfaceRecovery(
            host = host,
            generation = surfaceRecoveryTokens.currentToken,
            failedOperation = operation,
            failedResponse = response,
            retryAttempt = 1,
        )
    }

    private fun scheduleSurfaceRecovery(
        host: AndroidPlayerHost,
        generation: Long,
        failedOperation: String,
        failedResponse: NativeResponse,
        retryAttempt: Int,
    ) {
        if (disposed || boundHost !== host || !surfaceRecoveryTokens.isCurrent(generation)) {
            return
        }
        val delayMillis = androidSurfaceRecoveryDelayMillis(retryAttempt)
        if (delayMillis == null) {
            surfaceRecoveryRunnable = null
            plugin.reportSurfaceRecoveryExhausted(
                host = host,
                viewId = viewId,
                operation = failedOperation,
                generation = generation,
                retryAttempts = retryAttempt - 1,
                response = failedResponse,
            )
            plugin.onPlayerRenderStateChanged()
            return
        }

        Log.w(
            TAG,
            "surfaceRecoveryScheduled playerId=${host.handle} viewId=$viewId " +
                "operation=$failedOperation generation=$generation " +
                "retryAttempt=$retryAttempt delayMs=$delayMillis " +
                "status=${failedResponse.status} error=${failedResponse.error.orEmpty()}",
        )
        val runnable = Runnable {
            if (disposed || boundHost !== host || !surfaceRecoveryTokens.isCurrent(generation)) {
                return@Runnable
            }
            surfaceRecoveryRunnable = null
            val failure = performSurfaceRecovery(host)
            if (failure == null) {
                Log.i(
                    TAG,
                    "surfaceRecoverySucceeded playerId=${host.handle} viewId=$viewId " +
                        "generation=$generation retryAttempt=$retryAttempt",
                )
                if (
                    androidShouldRefreshHdrHeadroomAfterRecovery(
                        hostStillBound = boundHost === host,
                        surfaceAttached = host.surfaceAttached,
                        disposed = disposed,
                        disposeRequested = disposeRequested,
                        unbindRequested = unbindRequested,
                    )
                ) {
                    refreshHdrHeadroomObservation()
                }
            } else {
                scheduleSurfaceRecovery(
                    host = host,
                    generation = generation,
                    failedOperation = failure.operation,
                    failedResponse = failure.response,
                    retryAttempt = retryAttempt + 1,
                )
            }
            plugin.onPlayerRenderStateChanged()
        }
        surfaceRecoveryRunnable = runnable
        if (!mainHandler.postDelayed(runnable, delayMillis)) {
            surfaceRecoveryRunnable = null
            plugin.reportSurfaceRecoveryExhausted(
                host = host,
                viewId = viewId,
                operation = failedOperation,
                generation = generation,
                retryAttempts = retryAttempt - 1,
                response = failedResponse,
            )
        }
    }

    /** Returns the operation that still failed, or null once recovery is complete. */
    private fun performSurfaceRecovery(host: AndroidPlayerHost): SurfaceAttempt? {
        if (nativeDetachRetryPending || unbindRequested || disposeRequested) {
            val detachResponse = detachNativeSurface(host)
            plugin.reportSurfaceResponse(host, "detachSurface", detachResponse)
            if (!detachResponse.ok) {
                return SurfaceAttempt("detachSurface", detachResponse)
            }
            if (unbindRequested || disposeRequested) {
                completeUnbind(host)
                return null
            }
        }

        val attachAttempt = attachIfReady(host)
        plugin.reportSurfaceResponse(host, attachAttempt.operation, attachAttempt.response)
        return attachAttempt.takeUnless { it.response.ok }
    }

    private fun completeUnbind(host: AndroidPlayerHost) {
        val deferredBind = takePendingBind()
        stopHdrHeadroomObservation(publishUnknown = false)
        cancelSurfaceRecovery()
        nativeDetachRetryPending = false
        if (host.attachedView === this) {
            host.attachedView = null
        }
        if (boundHost === host) {
            boundHost = null
        }
        lastPublishedHdrHeadroom = null
        unbindRequested = false
        if (disposeRequested) {
            finishDispose()
        }
        resumePendingBind(deferredBind)
        plugin.onPlayerRenderStateChanged()
    }

    private fun queuePendingBind(host: AndroidPlayerHost, targetView: ErikaAndroidVideoView) {
        pendingBind = PendingViewBind(host, targetView)
        Log.w(
            TAG,
            "surfaceBindDeferred playerId=${host.handle} sourceViewId=$viewId " +
                "targetViewId=${targetView.viewId} reason=native_detach_recovery",
        )
    }

    private fun clearPendingBind() {
        pendingBind = null
    }

    private fun takePendingBind(): PendingViewBind? {
        val deferredBind = pendingBind
        pendingBind = null
        return deferredBind
    }

    private fun resumePendingBind(deferredBind: PendingViewBind?) {
        val pending = deferredBind ?: return
        val targetView = pending.targetView
        val targetAcceptsHost = targetView.boundHost == null ||
            targetView.boundHost === pending.host
        val hostAcceptsTarget = pending.host.attachedView == null ||
            pending.host.attachedView === targetView
        if (
            !androidShouldResumePendingViewBind(
                hostDestroyed = pending.host.isDestroyed,
                targetDisposed = targetView.disposed,
                targetDisposeRequested = targetView.disposeRequested,
                targetAcceptsHost = targetAcceptsHost,
                hostAcceptsTarget = hostAcceptsTarget,
            )
        ) {
            Log.i(
                TAG,
                "surfaceBindDeferredCancelled playerId=${pending.host.handle} " +
                    "sourceViewId=$viewId targetViewId=${targetView.viewId} " +
                    "hostDestroyed=${pending.host.isDestroyed} " +
                    "targetDisposed=${targetView.disposed} " +
                    "targetDisposeRequested=${targetView.disposeRequested} " +
                    "targetAcceptsHost=$targetAcceptsHost " +
                    "hostAcceptsTarget=$hostAcceptsTarget",
            )
            return
        }
        Log.i(
            TAG,
            "surfaceBindDeferredResume playerId=${pending.host.handle} " +
                "sourceViewId=$viewId targetViewId=${targetView.viewId}",
        )
        runCatching { targetView.bind(pending.host) }
            .onSuccess { response ->
                if (!response.ok) {
                    Log.w(
                        TAG,
                        "surfaceBindDeferredStillPending playerId=${pending.host.handle} " +
                            "sourceViewId=$viewId targetViewId=${targetView.viewId} " +
                            "status=${response.status} error=${response.error.orEmpty()}",
                    )
                }
            }
            .onFailure { error ->
                Log.e(
                    TAG,
                    "surfaceBindDeferredFailed playerId=${pending.host.handle} " +
                        "sourceViewId=$viewId targetViewId=${targetView.viewId}",
                    error,
                )
            }
    }

    private fun cancelSurfaceRecovery() {
        surfaceRecoveryTokens.invalidate()
        surfaceRecoveryRunnable?.let(mainHandler::removeCallbacks)
        surfaceRecoveryRunnable = null
    }

    private fun surfaceMetrics(pixelWidth: Int, pixelHeight: Int): SurfaceMetrics {
        // SurfaceTexture dimensions are already physical pixels. Passing an
        // artificial logical size and density through JNI caused a lossy
        // divide/round/multiply cycle on non-integral Android densities, so the
        // wgpu swapchain could differ from the actual buffer by one or more
        // pixels. Keep Android's surface contract pixel-exact.
        return SurfaceMetrics(
            width = max(1, pixelWidth),
            height = max(1, pixelHeight),
            scale = 1.0,
        )
    }

    private fun releaseSurface() {
        if (ownsOutputSurface) {
            outputSurface?.release()
        }
        outputSurface = null
        ownsOutputSurface = false
    }

    private data class SurfaceMetrics(
        val width: Int,
        val height: Int,
        val scale: Double,
    )

    private data class SurfaceAttempt(
        val operation: String,
        val response: NativeResponse,
    )

    private data class PendingViewBind(
        val host: AndroidPlayerHost,
        val targetView: ErikaAndroidVideoView,
    )

    private companion object {
        const val TAG = "ErikaAndroidVideoView"
    }
}
