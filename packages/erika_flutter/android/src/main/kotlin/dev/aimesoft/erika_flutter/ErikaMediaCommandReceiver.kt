package dev.aimesoft.erika_flutter

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

class ErikaMediaCommandReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            ErikaMediaSession.ACTION_PLAY -> commandHandler?.invoke(ErikaMediaSession.ACTION_PLAY)
            ErikaMediaSession.ACTION_PAUSE -> commandHandler?.invoke(ErikaMediaSession.ACTION_PAUSE)
            ErikaMediaSession.ACTION_STOP -> commandHandler?.invoke(ErikaMediaSession.ACTION_STOP)
        }
    }

    internal companion object {
        var commandHandler: ((String) -> Unit)? = null
    }
}
