package app.hocket.ui.a11y

import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsNode
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.junit4.ComposeTestRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.text.TextLayoutResult

/**
 * Accessibility checks over the Compose semantics tree, for Robolectric UI tests.
 *
 * Why not the Accessibility Test Framework (`enableAccessibilityChecks()` from
 * `ui-test-junit4-accessibility`): under Robolectric ATF builds its hierarchy from the View tree
 * and never reaches Compose's virtual nodes (verified: an unlabelled 10 dp clickable passes). These
 * are the ATF checks that matter here, applied to the merged semantics tree Compose hands TalkBack:
 * - speakable: every actionable node (click, long click, adjust, toggle) has a label;
 * - touch target: actionable nodes have a 48 x 48 dp touch area and a visible target of at least
 *   24 dp (a tiny control cannot rely on Compose's touch-area extension alone);
 * - redundant description: no "button" / "double tap" in a label (TalkBack adds the role/hint);
 * - duplicate clickable bounds: no two actionable nodes on exactly the same rectangle.
 * Plus the large-font layout checks ([layoutIssues]): no control or text clipped by its own
 * container (scroll viewports excepted), and no two controls overlapping.
 */
object A11yChecks {
    data class Issue(val node: String, val problem: String) {
        override fun toString() = "$problem: $node"
    }

    fun actionable(n: SemanticsNode): Boolean = with(n.config) {
        contains(SemanticsActions.OnClick) || contains(SemanticsActions.OnLongClick) || contains(SemanticsActions.SetProgress) || contains(SemanticsProperties.ToggleableState)
    }

    fun label(n: SemanticsNode): String = listOfNotNull(
        n.config.getOrNull(SemanticsProperties.ContentDescription)?.joinToString(" "),
        n.config.getOrNull(SemanticsProperties.Text)?.joinToString(" ") { it.text },
        n.config.getOrNull(SemanticsProperties.EditableText)?.text,
    ).joinToString(" ").trim()

    fun describe(n: SemanticsNode): String {
        val tag = n.config.getOrNull(SemanticsProperties.TestTag)
        return "#${n.id}${tag?.let { " '$it'" } ?: ""} \"${label(n).take(60)}\" ${n.boundsInRoot}"
    }

    fun all(root: SemanticsNode): List<SemanticsNode> = buildList {
        fun walk(n: SemanticsNode) { add(n); n.children.forEach(::walk) }
        walk(root)
    }

    private fun visible(n: SemanticsNode, root: Rect): Boolean {
        val b = n.boundsInRoot
        return b.width > 0.5f && b.height > 0.5f && b.overlaps(root) && !n.config.contains(SemanticsProperties.InvisibleToUser)
    }

    /** The node's own rectangle before any clipping by ancestors. */
    private fun unclipped(n: SemanticsNode): Rect = Rect(n.positionInRoot, androidx.compose.ui.geometry.Size(n.size.width.toFloat(), n.size.height.toFloat()))

    private fun isClipped(n: SemanticsNode): Boolean {
        val u = unclipped(n)
        val b = n.boundsInRoot
        return b.width < u.width - 1f || b.height < u.height - 1f
    }

    private fun scrollAncestor(n: SemanticsNode): SemanticsNode? {
        var p = n.parent
        while (p != null) {
            if (p.config.contains(SemanticsActions.ScrollBy) || p.config.contains(SemanticsProperties.VerticalScrollAxisRange) || p.config.contains(SemanticsProperties.HorizontalScrollAxisRange)) return p
            p = p.parent
        }
        return null
    }

    /** Clipped only where a scrolling ancestor's viewport cuts it (content scrolled partly out of view). */
    private fun clippedByScroll(n: SemanticsNode, rootBounds: Rect): Boolean {
        val u = unclipped(n)
        val b = n.boundsInRoot
        val scroller = scrollAncestor(n)?.boundsInRoot ?: return false
        fun near(a: Float, c: Float) = kotlin.math.abs(a - c) <= 2f
        val topCut = b.top > u.top + 1f
        val bottomCut = b.bottom < u.bottom - 1f
        val leftCut = b.left > u.left + 1f
        val rightCut = b.right < u.right - 1f
        return (!topCut || near(b.top, scroller.top) || near(b.top, rootBounds.top)) &&
            (!bottomCut || near(b.bottom, scroller.bottom) || near(b.bottom, rootBounds.bottom)) &&
            (!leftCut || near(b.left, scroller.left)) &&
            (!rightCut || near(b.right, scroller.right))
    }

    private fun isAncestor(a: SemanticsNode, b: SemanticsNode): Boolean {
        var p = b.parent
        while (p != null) { if (p.id == a.id) return true; p = p.parent }
        return false
    }

    fun issues(rule: ComposeTestRule, minTouchDp: Float = 48f): List<Issue> {
        val root = rule.onRoot().fetchSemanticsNode()
        val density = rule.density.density
        val rootBounds = root.boundsInRoot
        val nodes = all(root).filter { actionable(it) && visible(it, rootBounds) }
        val out = mutableListOf<Issue>()
        for (n in nodes) {
            val l = label(n)
            if (l.isEmpty()) out += Issue(describe(n), "no label")
            if (Regex("\\b(button|double[- ]tap)\\b", RegexOption.IGNORE_CASE).containsMatchIn(l)) out += Issue(describe(n), "redundant description")
            // A control scrolled half out of view is fine; judge size on fully visible ones: the touch
            // area (what Compose reports to accessibility services, extended by
            // minimumInteractiveComponentSize) must be 48 dp, and the visible target at least 24 dp
            // (WCAG 2.5.8), so a tiny control cannot hide behind the extension.
            if (!isClipped(n)) {
                val t = n.touchBoundsInRoot
                val b = n.boundsInRoot
                val min = minTouchDp * density - 1f
                if (t.width < min || t.height < min) out += Issue(describe(n) + " touch=${t.width / density}x${t.height / density}dp", "touch target below ${minTouchDp}dp")
                val visibleMin = 24f * density - 1f
                if (b.width < visibleMin || b.height < visibleMin) out += Issue(describe(n) + " visible=${b.width / density}x${b.height / density}dp", "target below 24dp")
            }
        }
        val byBounds = nodes.groupBy { it.boundsInRoot }
        byBounds.values.filter { it.size > 1 }.forEach { group ->
            val unrelated = group.filter { a -> group.none { b -> b !== a && (isAncestor(a, b) || isAncestor(b, a)) } }
            if (unrelated.size > 1) out += Issue(unrelated.joinToString { describe(it) }, "duplicate clickable bounds")
        }
        return out
    }

    /** Clipped or overlapping controls and clipped text (text shortened with an ellipsis is fine). */
    fun layoutIssues(rule: ComposeTestRule): List<Issue> {
        val root = rule.onRoot().fetchSemanticsNode()
        val rootBounds = root.boundsInRoot
        val everything = all(root).filter { visible(it, rootBounds) }
        val out = mutableListOf<Issue>()
        val controls = everything.filter { actionable(it) }
        for (n in controls) {
            if (isClipped(n) && !clippedByScroll(n, rootBounds)) out += Issue(describe(n) + " unclipped=${unclipped(n)}", "control clipped")
        }
        for (i in controls.indices) for (j in i + 1 until controls.size) {
            val a = controls[i]; val b = controls[j]
            if (isAncestor(a, b) || isAncestor(b, a)) continue
            val x = a.boundsInRoot.intersect(b.boundsInRoot)
            if (x.width > 1f && x.height > 1f) out += Issue(describe(a) + " / " + describe(b), "controls overlap")
        }
        for (n in everything) {
            if (!n.config.contains(SemanticsActions.GetTextLayoutResult)) continue
            val results = mutableListOf<TextLayoutResult>()
            n.config[SemanticsActions.GetTextLayoutResult].action?.invoke(results)
            val r = results.firstOrNull() ?: continue
            val ellipsized = r.lineCount > 0 && r.isLineEllipsized(r.lineCount - 1)
            if (r.didOverflowHeight && !ellipsized) out += Issue(describe(n), "text clipped")
            if (isClipped(n) && !clippedByScroll(n, rootBounds)) out += Issue(describe(n) + " unclipped=${unclipped(n)}", "text clipped by its container")
        }
        return out
    }

    fun assertAccessible(rule: ComposeTestRule, what: String, minTouchDp: Float = 48f) {
        val found = issues(rule, minTouchDp)
        if (found.isNotEmpty()) throw AssertionError("$what: ${found.size} accessibility issue(s):\n" + found.joinToString("\n"))
    }

    fun assertLayoutHolds(rule: ComposeTestRule, what: String) {
        val found = layoutIssues(rule)
        if (found.isNotEmpty()) throw AssertionError("$what: ${found.size} layout issue(s) at this font/display size:\n" + found.joinToString("\n"))
    }
}
