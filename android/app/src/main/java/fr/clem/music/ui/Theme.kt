package fr.clem.music.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/** Mêmes couleurs que l'app PC : ardoise et corail. */
val Corail = Color(0xFFFF5533)

private val Light = lightColorScheme(
    primary = Corail,
    onPrimary = Color.White,
    primaryContainer = Color(0xFFFFE3DC),
    onPrimaryContainer = Color(0xFF5C1606),
    secondary = Color(0xFF3A4152),
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFDDE2EA),
    onSecondaryContainer = Color(0xFF171C28),
    background = Color(0xFFEEF1F5),
    onBackground = Color(0xFF171C28),
    surface = Color(0xFFF7F8FA),
    onSurface = Color(0xFF171C28),
    surfaceVariant = Color(0xFFE2E6EC),
    onSurfaceVariant = Color(0xFF5E6575),
    surfaceContainer = Color(0xFFFFFFFF),
    surfaceContainerHigh = Color(0xFFFFFFFF),
    surfaceContainerHighest = Color(0xFFE6E9EF),
    surfaceContainerLow = Color(0xFFF4F6F9),
    outline = Color(0xFFC7CDD7),
    outlineVariant = Color(0xFFDDE1E8),
)

private val Dark = darkColorScheme(
    // Graphite neutre : gris sans teinte, comme sur le PC.
    primary = Corail,
    onPrimary = Color.White,
    primaryContainer = Color(0xFF4A1D12),
    onPrimaryContainer = Color(0xFFFFDBD1),
    secondary = Color(0xFFBDBDBD),
    onSecondary = Color(0xFF121212),
    secondaryContainer = Color(0xFF2A2A2D),
    onSecondaryContainer = Color(0xFFEDEDED),
    background = Color(0xFF121212),
    onBackground = Color(0xFFEDEDED),
    surface = Color(0xFF161616),
    onSurface = Color(0xFFEDEDED),
    surfaceVariant = Color(0xFF2A2A2D),
    onSurfaceVariant = Color(0xFF9E9EA3),
    surfaceContainer = Color(0xFF1C1C1E),
    surfaceContainerHigh = Color(0xFF232325),
    surfaceContainerHighest = Color(0xFF2A2A2D),
    surfaceContainerLow = Color(0xFF171717),
    outline = Color(0xFF3A3A3D),
    outlineVariant = Color(0xFF2C2C2F),
)

private val Type = Typography(
    headlineMedium = TextStyle(fontSize = 26.sp, fontWeight = FontWeight.Bold, letterSpacing = (-0.3).sp),
    titleLarge = TextStyle(fontSize = 20.sp, fontWeight = FontWeight.Bold),
    titleMedium = TextStyle(fontSize = 16.sp, fontWeight = FontWeight.SemiBold),
    bodyLarge = TextStyle(fontSize = 15.sp, fontWeight = FontWeight.Medium),
    bodyMedium = TextStyle(fontSize = 14.sp),
    bodySmall = TextStyle(fontSize = 12.sp),
    labelLarge = TextStyle(fontSize = 14.sp, fontWeight = FontWeight.SemiBold),
    labelMedium = TextStyle(fontSize = 12.sp, fontWeight = FontWeight.SemiBold, letterSpacing = 0.6.sp),
)

@Composable
fun MusicTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (isSystemInDarkTheme()) Dark else Light, typography = Type, content = content)
}
