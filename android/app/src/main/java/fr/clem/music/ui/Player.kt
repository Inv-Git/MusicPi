package fr.clem.music.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.PlaylistAdd
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.KeyboardArrowDown
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PhoneAndroid
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Repeat
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.SkipPrevious
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.SliderDefaults
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import fr.clem.music.Link
import fr.clem.music.I18n
import fr.clem.music.MainViewModel
import fr.clem.music.Now
import fr.clem.music.Target
import fr.clem.music.fmtPrecise
import fr.clem.music.fmtTime
import fr.clem.music.tr

@Composable
fun MiniPlayer(vm: MainViewModel, open: () -> Unit) {
    val now by vm.now.collectAsState()
    val index by vm.repo.index.collectAsState()
    val t = now.path?.let { index?.byPath?.get(it) }
    if (t == null && now.target == Target.Phone) return
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, tonalElevation = 2.dp) {
        Column {
            LinearProgressIndicator(
                progress = { if (now.duration > 0) (now.position / now.duration).toFloat().coerceIn(0f, 1f) else 0f },
                modifier = Modifier.fillMaxWidth().height(2.dp),
                drawStopIndicator = {},
                gapSize = 0.dp,
            )
            Row(
                verticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.fillMaxWidth().clickable(onClick = open).padding(horizontal = 12.dp, vertical = 8.dp),
            ) {
                TrackCover(vm, now.path, Modifier.size(44.dp))
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Text(t?.displayTitle ?: tr("Rien en lecture sur le PC"), style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (now.target == Target.Pc) {
                            Icon(Icons.Rounded.Computer, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(13.dp))
                            Spacer(Modifier.width(4.dp))
                        }
                        Text(
                            if (now.target == Target.Pc && !now.reachable) tr("PC injoignable") else t?.artist ?: "",
                            style = MaterialTheme.typography.bodySmall,
                            color = if (now.target == Target.Pc) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                IconButton(onClick = vm::toggle) {
                    Icon(if (now.playing) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, if (now.playing) "Pause" else tr("Lecture"), modifier = Modifier.size(30.dp))
                }
                IconButton(onClick = vm::next) { Icon(Icons.Rounded.SkipNext, tr("Suivant")) }
            }
        }
    }
}

@Composable
fun FullPlayer(vm: MainViewModel, close: () -> Unit, openAlbum: (String) -> Unit, toast: (String) -> Unit) {
    val now by vm.now.collectAsState()
    val index by vm.repo.index.collectAsState()
    val link by vm.repo.link.collectAsState()
    val t = now.path?.let { index?.byPath?.get(it) }
    var dragging by remember { mutableStateOf<Float?>(null) }
    var practiceOpen by remember { mutableStateOf(now.speed != 100 || now.loop != null) }

    Surface(color = MaterialTheme.colorScheme.background, modifier = Modifier.fillMaxSize()) {
        Column(
            Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding().verticalScroll(rememberScrollState()).padding(horizontal = 24.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth().padding(vertical = 8.dp)) {
                IconButton(onClick = close) { Icon(Icons.Rounded.KeyboardArrowDown, tr("Fermer"), modifier = Modifier.size(30.dp)) }
                Spacer(Modifier.weight(1f))
                TargetChip(Icons.Rounded.PhoneAndroid, tr("Téléphone"), now.target == Target.Phone) { vm.switchTarget(Target.Phone) }
                Spacer(Modifier.width(8.dp))
                TargetChip(Icons.Rounded.Computer, vm.repo.prefs.pcName.ifBlank { "PC" }, now.target == Target.Pc, enabled = link == Link.Online) {
                    vm.switchTarget(Target.Pc)
                }
            }
            Spacer(Modifier.height(12.dp))
            TrackCover(vm, now.path, Modifier.fillMaxWidth().aspectRatio(1f), size = 800, corner = 14)
            Spacer(Modifier.height(24.dp))
            val coll by vm.collections.collectAsState()
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    t?.displayTitle ?: if (now.target == Target.Pc) tr("Rien en lecture sur le PC") else "",
                    style = MaterialTheme.typography.titleLarge,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (t != null) {
                    var adding by remember { mutableStateOf(false) }
                    if (adding) AddToPlaylistDialog(vm, listOf(t.path), toast) { adding = false }
                    IconButton(onClick = { adding = true }) { Icon(Icons.AutoMirrored.Rounded.PlaylistAdd, tr("Ajouter à une playlist")) }
                    HeartButton(t.path in coll.likedTracks) { vm.toggleLike(t.path) }
                }
            }
            Text(
                listOfNotNull(t?.artist, t?.album).joinToString(" · "),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.clickable(enabled = t != null) { t?.let { openAlbum(it.path) } },
            )
            if (t != null && t.quality.isNotBlank()) {
                Text(t.quality, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = 4.dp))
            }
            if (now.target == Target.Pc && !now.reachable) {
                Text(tr("PC injoignable : vérifie qu'il est allumé et que MusicPi est ouvert."), color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(top = 8.dp))
            }
            Spacer(Modifier.height(16.dp))

            // Barre de progression, avec la boucle si elle est posée.
            val dur = now.duration.coerceAtLeast(0.001)
            Slider(
                value = dragging ?: (now.position / dur).toFloat().coerceIn(0f, 1f),
                onValueChange = { dragging = it },
                onValueChangeFinished = { dragging?.let { vm.seek(it * dur) }; dragging = null },
                colors = SliderDefaults.colors(thumbColor = MaterialTheme.colorScheme.primary, activeTrackColor = MaterialTheme.colorScheme.primary),
            )
            Row(Modifier.fillMaxWidth()) {
                Text(fmtTime(dragging?.let { it * dur } ?: now.position), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.weight(1f))
                now.loop?.let {
                    Text((if (I18n.en) "Loop " else "Boucle ") + "${fmtTime(it.first)} → ${fmtTime(it.second)}", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
                    Spacer(Modifier.weight(1f))
                }
                Text(fmtTime(now.duration), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically, modifier = Modifier.fillMaxWidth()) {
                IconButton(onClick = vm::prev, modifier = Modifier.size(56.dp)) { Icon(Icons.Rounded.SkipPrevious, tr("Précédent"), modifier = Modifier.size(36.dp)) }
                FilledIconButton(
                    onClick = vm::toggle,
                    modifier = Modifier.size(72.dp),
                    colors = IconButtonDefaults.filledIconButtonColors(containerColor = MaterialTheme.colorScheme.primary),
                ) { Icon(if (now.playing) Icons.Rounded.Pause else Icons.Rounded.PlayArrow, tr("Lecture / pause"), modifier = Modifier.size(40.dp)) }
                IconButton(onClick = vm::next, modifier = Modifier.size(56.dp)) { Icon(Icons.Rounded.SkipNext, tr("Suivant"), modifier = Modifier.size(36.dp)) }
            }

            Spacer(Modifier.height(16.dp))
            TextButton(onClick = { practiceOpen = !practiceOpen }) {
                Icon(Icons.Rounded.Speed, null)
                Spacer(Modifier.width(8.dp))
                Text(if (practiceOpen) tr("Masquer le mode travail") else tr("Mode travail : vitesse et boucle"))
            }
            if (practiceOpen) PracticeCard(vm, now, toast)
            Spacer(Modifier.height(24.dp))
        }
    }
}

@Composable
private fun TargetChip(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, selected: Boolean, enabled: Boolean = true, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        enabled = enabled,
        label = { Text(label, maxLines = 1) },
        leadingIcon = { Icon(icon, null, modifier = Modifier.size(18.dp)) },
        colors = FilterChipDefaults.filterChipColors(
            selectedContainerColor = MaterialTheme.colorScheme.primary,
            selectedLabelColor = MaterialTheme.colorScheme.onPrimary,
            selectedLeadingIconColor = MaterialTheme.colorScheme.onPrimary,
        ),
    )
}

@Composable
private fun PracticeCard(vm: MainViewModel, now: Now, toast: (String) -> Unit) {
    val pendingA by vm.loopStart.collectAsState()
    var speed by remember { mutableStateOf<Float?>(null) }
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = RoundedCornerShape(16.dp), modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(tr("VITESSE"), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                val shown = speed?.toInt() ?: now.speed
                Text(
                    "$shown %",
                    style = MaterialTheme.typography.titleMedium,
                    color = if (shown != 100) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.clip(RoundedCornerShape(8.dp)).clickable { vm.setSpeed(100) }.padding(horizontal = 8.dp, vertical = 4.dp),
                )
            }
            if (!now.speedAvailable) {
                Text(tr("Le module de vitesse (bass_fx.dll) manque sur le PC."), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                StepButton("−", now.speedAvailable) { vm.setSpeed(now.speed - 5) }
                Slider(
                    value = speed ?: now.speed.toFloat(),
                    onValueChange = { speed = it },
                    onValueChangeFinished = { speed?.let { vm.setSpeed(it.toInt()) }; speed = null },
                    valueRange = 50f..150f,
                    steps = 19,
                    enabled = now.speedAvailable,
                    modifier = Modifier.weight(1f).padding(horizontal = 8.dp),
                )
                StepButton("+", now.speedAvailable) { vm.setSpeed(now.speed + 5) }
            }
            Text(tr("La hauteur des notes ne change pas."), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)

            Spacer(Modifier.height(16.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Rounded.Repeat, null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text(tr("BOUCLE"), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                val a = pendingA ?: now.loop?.first
                val b = now.loop?.second
                LoopButton(tr("Début"), a, Modifier.weight(1f)) { vm.markLoopStart() }
                LoopButton(tr("Fin"), b, Modifier.weight(1f)) { vm.markLoopEnd()?.let(toast) }
                IconButton(onClick = vm::clearLoop, enabled = a != null || b != null) { Icon(Icons.Rounded.Close, tr("Effacer la boucle")) }
            }
            Text(
                tr("Touche « Début » au début du passage, puis « Fin » : il tourne en boucle jusqu'à ce que tu l'effaces."),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
    }
}

@Composable
private fun StepButton(label: String, enabled: Boolean, onClick: () -> Unit) {
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier.size(36.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest)
            .clickable(enabled = enabled, onClick = onClick),
    ) { Text(label, style = MaterialTheme.typography.titleMedium) }
}

@Composable
private fun LoopButton(label: String, value: Double?, modifier: Modifier, onClick: () -> Unit) {
    OutlinedButton(onClick = onClick, modifier = modifier) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.width(8.dp))
        Text(value?.let { fmtPrecise(it) } ?: "—", color = if (value != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface)
    }
}
