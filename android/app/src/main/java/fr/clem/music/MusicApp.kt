package fr.clem.music

import android.app.Application

class MusicApp : Application() {
    lateinit var repo: Repo
        private set
    lateinit var downloads: Downloads
        private set

    override fun onCreate() {
        super.onCreate()
        repo = Repo(this, Prefs(this))
        downloads = Downloads(this, repo)
        repo.reconnect()
    }
}
