package cc.off42.office.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// The off42 brand (website/public/assets/favicon.svg): deep green, cream paper and a warm orange accent.
private val Green = Color(0xFF173B30)
private val GreenLight = Color(0xFF2F6B57)
private val Cream = Color(0xFFFFF9EF)
private val Orange = Color(0xFFFFAB80)
private val OrangeDeep = Color(0xFF9A4A22)

private val Light = lightColorScheme(
    primary = Green,
    onPrimary = Cream,
    primaryContainer = Color(0xFFCFE8DC),
    onPrimaryContainer = Color(0xFF0B241C),
    secondary = OrangeDeep,
    onSecondary = Color.White,
    secondaryContainer = Color(0xFFFFDBCB),
    onSecondaryContainer = Color(0xFF351000),
    tertiary = GreenLight,
    background = Cream,
    onBackground = Color(0xFF1C1B18),
    surface = Cream,
    onSurface = Color(0xFF1C1B18),
    surfaceVariant = Color(0xFFEDE6DA),
    onSurfaceVariant = Color(0xFF4A463F),
    surfaceContainer = Color(0xFFF6EFE4),
    surfaceContainerLow = Color(0xFFFBF4E9),
    surfaceContainerHigh = Color(0xFFF0E9DE),
    outline = Color(0xFF7B766C),
)

private val Dark = darkColorScheme(
    primary = Color(0xFF9FD3BD),
    onPrimary = Color(0xFF00382A),
    primaryContainer = Color(0xFF1F4D3E),
    onPrimaryContainer = Color(0xFFCFE8DC),
    secondary = Orange,
    onSecondary = Color(0xFF4A1C05),
    secondaryContainer = Color(0xFF6B3315),
    onSecondaryContainer = Color(0xFFFFDBCB),
    tertiary = Color(0xFF8CC7AE),
    background = Color(0xFF0E1F19),
    onBackground = Color(0xFFE7E2D8),
    surface = Color(0xFF0E1F19),
    onSurface = Color(0xFFE7E2D8),
    surfaceVariant = Color(0xFF2A3A33),
    onSurfaceVariant = Color(0xFFC3CCC6),
    surfaceContainer = Color(0xFF16291F),
    surfaceContainerLow = Color(0xFF122420),
    surfaceContainerHigh = Color(0xFF1D3229),
    outline = Color(0xFF8C9790),
)

/** Follows the system light/dark setting. */
@Composable
fun Off42Theme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = if (dark) Dark else Light, content = content)
}
