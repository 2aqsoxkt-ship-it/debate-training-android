package com.debate.training.android.platform

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import org.json.JSONObject
import java.util.UUID

/** Native-only API: these methods are not exposed to JavaScript. */
object PlatformBridge {
    private const val ALIAS = "debate.deepseek.aes.v1"
    private const val PREFS = "debate-secure-credential-v1"

    @JvmStatic @Suppress("UNUSED_PARAMETER")
    fun capabilities(context: Context, argument: String): String = JSONObject()
        .put("platform", "android")
        .put("sdk", Build.VERSION.SDK_INT)
        .put("secureStore", "AndroidKeyStore-AES-GCM")
        .put("backgroundAvailable", false)
        .put("hasApiKey", context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).contains("ciphertext"))
        .toString()

    private fun encryptionKey(alias: String = ALIAS): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        val existing = store.getKey(alias, null)
        if (existing is SecretKey) return existing
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(KeyGenParameterSpec.Builder(alias, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setRandomizedEncryptionRequired(true)
            .build())
        return generator.generateKey()
    }

    @JvmStatic @Synchronized
    fun saveKey(context: Context, argument: String): String {
        require(argument.isNotEmpty() && argument.none { it.isWhitespace() })
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, encryptionKey())
        val encrypted = cipher.doFinal(argument.toByteArray(Charsets.UTF_8))
        check(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putInt("version", 1)
            .putString("iv", Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .putString("ciphertext", Base64.encodeToString(encrypted, Base64.NO_WRAP)).commit())
        return "ok"
    }

    @JvmStatic @Synchronized @Suppress("UNUSED_PARAMETER")
    fun readKey(context: Context, argument: String): String {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val text = prefs.getString("ciphertext", null) ?: return ""
        check(prefs.getInt("version", 0) == 1)
        val iv = Base64.decode(checkNotNull(prefs.getString("iv", null)), Base64.NO_WRAP)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        val key = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.getKey(ALIAS, null)
        check(key is SecretKey) { "Secure credential unavailable" }
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, iv))
        return cipher.doFinal(Base64.decode(text, Base64.NO_WRAP)).toString(Charsets.UTF_8)
    }

    @JvmStatic @Synchronized @Suppress("UNUSED_PARAMETER")
    fun clearKey(context: Context, argument: String): String {
        check(context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().clear().commit())
        KeyStore.getInstance("AndroidKeyStore").apply { load(null); deleteEntry(ALIAS) }
        return "ok"
    }

    @JvmStatic @Suppress("UNUSED_PARAMETER")
    fun selfTest(context: Context, argument: String): String {
        val alias = "debate.keystore.proof." + UUID.randomUUID().toString()
        try {
            val key = encryptionKey(alias)
            val original = "安卓安全存储测试".toByteArray(Charsets.UTF_8)
            val encoder = Cipher.getInstance("AES/GCM/NoPadding")
            encoder.init(Cipher.ENCRYPT_MODE, key)
            val encrypted = encoder.doFinal(original)
            val decoder = Cipher.getInstance("AES/GCM/NoPadding")
            decoder.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, encoder.iv))
            check(original.contentEquals(decoder.doFinal(encrypted)))
            val tampered = encrypted.clone().apply { this[0] = (this[0].toInt() xor 1).toByte() }
            decoder.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, encoder.iv))
            var rejected = false
            try { decoder.doFinal(tampered) } catch (_: javax.crypto.AEADBadTagException) { rejected = true }
            check(rejected)
            return "ok"
        } finally {
            KeyStore.getInstance("AndroidKeyStore").apply { load(null); deleteEntry(alias) }
        }
    }
}
