package dev.playhorizon.mobile.account

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Ely.by Yggdrasil login. Password is used only for the HTTPS request and is
 * never written to disk. The returned access token is encrypted with an
 * Android Keystore AES-GCM key while the app is closed.
 */
class ElyByAccountStore(private val directory: File) {
    companion object {
        private const val AUTH_URL = "https://authserver.ely.by/auth/authenticate"
        private const val KEY_ALIAS = "horizon_elyby_account_v1"
        private const val ACCOUNT_FILE = "elyby-account.enc"
        private const val SESSION_FILE = "horizon-session.json"
    }

    data class Profile(val name: String, val uuid: String)

    fun signIn(username: String, password: CharArray, twoFactorCode: String? = null): Profile {
        try {
            require(username.isNotBlank()) { "Informe o usuário ou e-mail Ely.by." }
            require(password.isNotEmpty()) { "Informe a senha Ely.by." }
            val clientToken = java.util.UUID.randomUUID().toString()
            val body = JSONObject().apply {
                put("agent", JSONObject().put("name", "Minecraft").put("version", 1))
                put("username", username)
                val basePassword = String(password)
                val code = twoFactorCode?.trim().orEmpty()
                require(code.isEmpty() || code.matches(Regex("[0-9]{6}"))) {
                    "O código 2FA deve conter 6 números."
                }
                put("password", if (code.isEmpty()) basePassword else "$basePassword:$code")
                put("clientToken", clientToken)
                put("requestUser", true)
            }.toString()
            val connection = (URL(AUTH_URL).openConnection() as HttpURLConnection).apply {
                requestMethod = "POST"
                instanceFollowRedirects = false
                connectTimeout = 15000
                readTimeout = 20000
                doOutput = true
                setRequestProperty("Content-Type", "application/json; charset=utf-8")
                setRequestProperty("Accept", "application/json")
            }
            val response = try {
                connection.outputStream.use { it.write(body.toByteArray(Charsets.UTF_8)) }
                val status = connection.responseCode
                val stream = if (status in 200..299) connection.inputStream else connection.errorStream
                val responseText = stream?.bufferedReader(Charsets.UTF_8)?.use { it.readText() }.orEmpty()
                if (status !in 200..299) {
                    val message = runCatching { JSONObject(responseText).optString("errorMessage") }.getOrNull()
                    error(message?.takeIf { it.isNotBlank() } ?: "Ely.by recusou o login (HTTP $status).")
                }
                JSONObject(responseText)
            } finally {
                connection.disconnect()
            }
            val token = response.optString("accessToken").takeIf { it.isNotBlank() }
                ?: error("Ely.by não retornou um token de sessão.")
            val profile = response.optJSONObject("selectedProfile")
                ?: error("Ely.by não retornou o perfil Minecraft.")
            val name = profile.optString("name")
            val uuid = profile.optString("id").replace("-", "")
            require(name.matches(Regex("[A-Za-z0-9_]{3,16}"))) { "Nome de perfil Ely.by inválido." }
            require(uuid.matches(Regex("[a-fA-F0-9]{32}"))) { "UUID de perfil Ely.by inválido." }
            val returnedClientToken = response.optString("clientToken").takeIf { it.isNotBlank() } ?: clientToken
            val account = JSONObject()
                .put("name", name)
                .put("uuid", uuid)
                .put("access_token", token)
                .put("client_token", returnedClientToken)
            writeEncrypted(account.toString().toByteArray(Charsets.UTF_8))
            return Profile(name, uuid)
        } finally {
            password.fill('\u0000')
        }
    }

    /** Creates a fresh, one-use session file immediately before NativeActivity starts. */
    fun createLaunchSessionFile(): File {
        val plain = decrypt(readEncrypted())
        val account = JSONObject(String(plain, Charsets.UTF_8))
        val session = JSONObject()
            .put("name", account.getString("name"))
            .put("uuid", account.getString("uuid"))
            .put("access_token", account.getString("access_token"))
            .put("user_type", "ely.by")
        if (!directory.exists() && !directory.mkdirs()) error("Não foi possível preparar a pasta privada.")
        val target = File(directory, SESSION_FILE)
        val temporary = File(directory, "$SESSION_FILE.part")
        temporary.writeText(session.toString(), Charsets.UTF_8)
        temporary.setReadable(false, false)
        temporary.setWritable(false, false)
        temporary.setReadable(true, true)
        temporary.setWritable(true, true)
        if (target.exists() && !target.delete()) error("Não foi possível substituir a sessão temporária.")
        if (!temporary.renameTo(target)) error("Não foi possível criar a sessão temporária.")
        return target
    }

    /** Removes the encrypted account, pending one-use session and Keystore key. */
    fun signOut() {
        listOf(ACCOUNT_FILE, "$ACCOUNT_FILE.part", SESSION_FILE, "$SESSION_FILE.part").forEach { name ->
            val file = File(directory, name)
            if (file.exists() && !file.delete()) {
                error("Não foi possível remover os dados locais da conta Ely.by.")
            }
        }
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (store.containsAlias(KEY_ALIAS)) store.deleteEntry(KEY_ALIAS)
    }

    fun hasAccount(): Boolean = File(directory, ACCOUNT_FILE).isFile

    private fun writeEncrypted(plain: ByteArray) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val encrypted = cipher.doFinal(plain)
        val bytes = cipher.iv + encrypted
        val target = File(directory, ACCOUNT_FILE)
        val temporary = File(directory, "$ACCOUNT_FILE.part")
        temporary.writeBytes(bytes)
        temporary.setReadable(false, false)
        temporary.setWritable(false, false)
        temporary.setReadable(true, true)
        temporary.setWritable(true, true)
        if (target.exists() && !target.delete()) error("Não foi possível atualizar a conta Ely.by.")
        if (!temporary.renameTo(target)) error("Não foi possível salvar a conta Ely.by.")
    }

    private fun readEncrypted(): ByteArray {
        val file = File(directory, ACCOUNT_FILE)
        require(file.isFile) { "Nenhuma conta Ely.by foi conectada." }
        val bytes = file.readBytes()
        require(bytes.size > 12) { "O arquivo seguro da conta Ely.by está inválido." }
        return bytes
    }

    private fun decrypt(bytes: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, bytes.copyOfRange(0, 12)))
        return cipher.doFinal(bytes.copyOfRange(12, bytes.size))
    }

    private fun key(): SecretKey {
        val store = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        (store.getKey(KEY_ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore")
        generator.init(
            KeyGenParameterSpec.Builder(
                KEY_ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setRandomizedEncryptionRequired(true)
                .build()
        )
        return generator.generateKey()
    }
}
