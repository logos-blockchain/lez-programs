pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import "../components/liquidity"
import "../components/shared"
import "../components/liquidity/AmountMath.js" as AmountMath
import "../components/shared/TokenVisuals.js" as TokenVisuals

// Detail view for a single pool, opened from PoolsPage. The pair, fee tier and
// token definition ids come from the clicked pool row (AMM_POOLS_CONFIG); the
// live numbers come from AmmUiBackend::resolvePoolAccount(), which is the only
// pool read the program exposes. Volume and transaction history are deliberately
// absent: the AMM stores no history, so there is nothing to chart.
Item {
    id: root

    objectName: "poolDetailPage"

    // Real backend replica (logos.module("amm_ui")) and the watch runtime,
    // wired from Main.qml. Null until the app is ready.
    property var backend: null
    property var runtime: null

    // The pool row that was activated in PoolsPage: { tokenA, tokenB, feeBps,
    // poolId, tokenADefinitionId, tokenBDefinitionId }. Null when nothing is open.
    property var pool: null

    signal backRequested()
    signal swapRequested(var pool)
    signal addLiquidityRequested(var pool)

    readonly property int pageMargin: width < 640 ? 16 : 24
    readonly property int contentMaxWidth: 960
    // Two columns only when the sidebar still gets a usable width next to the
    // balances card; below that everything stacks.
    readonly property bool wideLayout: root.width >= 900

    // ── Pool row (config) ────────────────────────────────────────────────────
    readonly property string symbolA: root.pool ? String(root.pool.tokenA || "") : ""
    readonly property string symbolB: root.pool ? String(root.pool.tokenB || "") : ""
    readonly property string definitionIdA: root.pool ? String(root.pool.tokenADefinitionId || "") : ""
    readonly property string definitionIdB: root.pool ? String(root.pool.tokenBDefinitionId || "") : ""
    readonly property string pairText: qsTr("%1 / %2").arg(root.symbolA).arg(root.symbolB)

    // ── Chain state (backend.resolvePoolAccount) ─────────────────────────────
    property bool chainLoading: false
    property bool chainResolved: false
    property bool poolExists: false
    property string poolError: ""
    property string reserveA: "0"
    property string reserveB: "0"
    property string liquiditySupply: "0"
    property string resolvedPoolId: ""
    property string lpDefinitionId: ""
    property string vaultAId: ""
    property string vaultBId: ""
    // -1 until the chain answers, so a legitimate zero-fee pool is distinguishable
    // from "not resolved yet" and the config fee can stand in meanwhile.
    property int chainFeeBps: -1

    // Monotonic tag for resolvePoolAccount requests: opening pool X, going back
    // and opening pool Y can leave X's reply in flight, and it must not paint
    // over Y's numbers.
    property int resolveGeneration: 0

    readonly property int feeBps: root.chainFeeBps >= 0
                                  ? root.chainFeeBps
                                  : (root.pool ? (Number(root.pool.feeBps) || 0) : 0)
    readonly property string poolId: root.resolvedPoolId.length > 0
                                     ? root.resolvedPoolId
                                     : (root.pool ? String(root.pool.poolId || "") : "")

    // ── Wallet position (backend.tokenHoldings) ──────────────────────────────
    // Loaded alongside the pool so the actions know whether there is anything to
    // withdraw. Every add mints into a fresh LP account, so a pool's position can
    // span several holdings: lpBalanceTotal is the position, lpHolding* is the one
    // account a withdrawal can burn from.
    property var holdings: []
    property string lpBalanceTotal: "0"
    property string lpHoldingId: ""
    property string lpHoldingBalance: "0"
    property string holdingAId: ""
    property string holdingBId: ""
    property int holdingsGeneration: 0

    readonly property bool hasPosition: AmountMath.isUnsigned(root.lpBalanceTotal)
                                        && AmountMath.normalize(root.lpBalanceTotal) !== "0"
    // Withdrawing needs the LP account to burn and both receiving holdings.
    readonly property bool canRemoveLiquidity: root.hasPosition
                                               && root.lpHoldingId.length > 0
                                               && root.holdingAId.length > 0
                                               && root.holdingBId.length > 0

    readonly property bool hasDefinitionIds: root.definitionIdA.length > 0
                                             && root.definitionIdB.length > 0
    // Both actions need the pair's definition ids to preselect anything;
    // swapping additionally needs a pool that actually holds reserves.
    readonly property bool canSwap: root.hasDefinitionIds && root.hasLiquidity
    readonly property bool canAddLiquidity: root.hasDefinitionIds

    function loadPoolState() {
        // Drop the previous pool's numbers first: this runs on every pool change,
        // and stale reserves next to a new pair's header would be wrong, not just late.
        root.chainLoading = false
        root.chainResolved = false
        root.poolExists = false
        root.poolError = ""
        root.reserveA = "0"
        root.reserveB = "0"
        root.liquiditySupply = "0"
        root.resolvedPoolId = ""
        root.lpDefinitionId = ""
        root.vaultAId = ""
        root.vaultBId = ""
        root.chainFeeBps = -1

        if (!root.backend || !root.runtime || !root.pool)
            return

        // Read the ids off the pool row rather than through definitionIdA/B: this
        // runs from onPoolChanged, and those bindings may not have re-evaluated
        // against the new row yet.
        const idA = String(root.pool.tokenADefinitionId || "")
        const idB = String(root.pool.tokenBDefinitionId || "")
        if (idA.length === 0 || idB.length === 0) {
            // A config entry without token definition ids can't be resolved or
            // acted on; say so instead of spinning forever.
            console.warn("pool row has no token definition ids:", JSON.stringify(root.pool))
            root.chainResolved = true
            root.poolError = qsTr("This pool is missing token definition ids in the pool config.")
            return
        }

        const generation = ++root.resolveGeneration
        root.chainLoading = true
        root.runtime.watch(root.backend.resolvePoolAccount(idA, idB),
            function(result) {
                if (generation !== root.resolveGeneration)
                    return
                root.chainLoading = false
                root.chainResolved = true
                root.poolExists = !!(result && result.status === "ok")
                root.reserveA = (result && result.reserveA) || "0"
                root.reserveB = (result && result.reserveB) || "0"
                root.liquiditySupply = (result && result.liquiditySupply) || "0"
                root.resolvedPoolId = (result && result.poolId) || ""
                root.lpDefinitionId = (result && result.lpDefinitionId) || ""
                root.vaultAId = (result && result.vaultAId) || ""
                root.vaultBId = (result && result.vaultBId) || ""
                root.chainFeeBps = (result && result.feeBps !== undefined) ? result.feeBps : -1
                // no_pool is the ordinary "not created yet" state, not a failure.
                root.poolError = (result && result.error && result.error !== "no_pool")
                                 ? root.issueText(result.error) : ""
            },
            function(error) {
                if (generation !== root.resolveGeneration)
                    return
                console.warn("resolvePoolAccount error:", error)
                root.chainLoading = false
                root.chainResolved = true
                root.poolExists = false
                root.poolError = qsTr("Failed to load pool state: %1").arg(error)
            })
    }

    // tokenHoldings() needs an open wallet and is independent of the pool read, so
    // it runs on its own and just refines the actions once it lands.
    function loadHoldings() {
        root.holdings = []
        root.lpBalanceTotal = "0"
        root.lpHoldingId = ""
        root.lpHoldingBalance = "0"
        root.holdingAId = ""
        root.holdingBId = ""

        if (!root.backend || !root.runtime || !root.pool || !root.backend.isWalletOpen)
            return

        const generation = ++root.holdingsGeneration
        root.runtime.watch(root.backend.tokenHoldings(),
            function(list) {
                if (generation !== root.holdingsGeneration)
                    return
                root.holdings = list || []
                root.applyHoldings()
            },
            function(err) {
                if (generation !== root.holdingsGeneration)
                    return
                console.warn("tokenHoldings error:", err)
            })
    }

    // Matches a holding on whichever encoding the id uses: tokenHoldings emits a
    // base58 definitionId and a hex definitionIdHex per row, while the configured
    // token ids and the pool's lpDefinitionId can be either (see TokenInput).
    function holdingsFor(definitionId) {
        var out = []
        if (definitionId.length === 0)
            return out
        var isHex = /^[0-9a-fA-F]{64}$/.test(definitionId)
        var field = isHex ? "definitionIdHex" : "definitionId"
        var needle = isHex ? definitionId.toLowerCase() : definitionId
        for (var i = 0; i < root.holdings.length; ++i) {
            var holding = root.holdings[i]
            var value = String(holding[field] || "")
            if ((isHex ? value.toLowerCase() : value) === needle)
                out.push(holding)
        }
        return out
    }

    function applyHoldings() {
        // The LP side: total across every holding is the position, but a burn names
        // one account, so the largest is the one a withdrawal can draw on.
        var lpHoldings = root.holdingsFor(root.lpDefinitionId)
        var total = "0"
        var best = null
        for (var i = 0; i < lpHoldings.length; ++i) {
            var balance = String(lpHoldings[i].balanceRaw || "0")
            if (!AmountMath.isUnsigned(balance))
                continue
            total = AmountMath.add(total, balance)
            if (!best || AmountMath.compare(balance, String(best.balanceRaw || "0")) > 0)
                best = lpHoldings[i]
        }
        root.lpBalanceTotal = total
        root.lpHoldingId = best ? String(best.accountId || "") : ""
        root.lpHoldingBalance = best ? String(best.balanceRaw || "0") : "0"

        // The receiving side: any existing holding for each token will do.
        var holdingsA = root.holdingsFor(root.definitionIdA)
        var holdingsB = root.holdingsFor(root.definitionIdB)
        root.holdingAId = holdingsA.length > 0 ? String(holdingsA[0].accountId || "") : ""
        root.holdingBId = holdingsB.length > 0 ? String(holdingsB[0].accountId || "") : ""
    }

    function refresh() {
        root.loadPoolState()
        root.loadHoldings()
    }

    onPoolChanged: root.refresh()
    onBackendChanged: root.refresh()
    onRuntimeChanged: root.refresh()

    // The LP definition only arrives with the pool read, which can land after the
    // holdings; re-match when it does.
    onLpDefinitionIdChanged: root.applyHoldings()

    Connections {
        target: root.backend
        function onIsWalletOpenChanged() { root.loadHoldings() }
    }

    function issueText(code) {
        switch (String(code)) {
        case "no_program_bin":
            return qsTr("The AMM program binary is not available to the app.")
        case "amm_not_initialized":
            return qsTr("The AMM config account has not been initialized.")
        case "bad_config":
            return qsTr("The AMM app configuration is invalid.")
        case "same_token_pair":
            return qsTr("This pool is configured with the same token on both sides.")
        default:
            return qsTr("Failed to load pool state: %1").arg(String(code))
        }
    }

    // ── Derived pool figures ─────────────────────────────────────────────────
    // Amounts are raw token units throughout the app (the liquidity flow quotes
    // and submits in raw units too), so they are shown unscaled here as well.
    readonly property real reserveANumber: Number(root.reserveA) || 0
    readonly property real reserveBNumber: Number(root.reserveB) || 0
    readonly property real supplyNumber: Number(root.liquiditySupply) || 0
    readonly property bool hasLiquidity: root.poolExists
                                         && root.reserveANumber > 0
                                         && root.reserveBNumber > 0

    // Share of the token units held on each side. Valued at the pool's own spot
    // price the two sides are equal by construction (reserveA * reserveB/reserveA
    // == reserveB), so a value-weighted bar would be a constant 50/50; the unit
    // split is what actually says something about the pair. Valuing them against
    // an external price would need a feed this page doesn't have.
    readonly property real balanceShareA: root.hasLiquidity
        ? root.reserveANumber / (root.reserveANumber + root.reserveBNumber) : 0.5
    readonly property real balanceShareB: 1 - root.balanceShareA

    // Fee growth, derived — the program keeps no fee accumulator. Swap fees stay
    // in the reserves and lift k without minting LP, so sqrt(k) / lpSupply starts
    // at exactly 1 (new_definition mints floor(sqrt(a*b)), including the locked
    // MINIMUM_LIQUIDITY) and only grows with collected fees. add mints
    // min(floor(L*dA/rA), floor(L*dB/rB)) and remove pays floor(lp*r/L), both
    // proportional, so neither can lower the ratio — it only ever rises. Called an
    // estimate because it also absorbs that floor rounding and the over-deposit an
    // imbalanced add leaves behind, not swap fees alone.
    readonly property real feeGrowth: {
        if (!root.hasLiquidity || root.supplyNumber <= 0)
            return 0
        // sqrt(a) * sqrt(b) rather than sqrt(a * b): the product of two u128-scale
        // reserves overflows a double long before either factor does.
        var growth = (Math.sqrt(root.reserveANumber) * Math.sqrt(root.reserveBNumber))
                     / root.supplyNumber
        return growth > 1 ? growth - 1 : 0
    }
    // The fraction of today's reserves that is collected fees rather than deposits.
    readonly property real accruedShare: root.feeGrowth > 0
                                         ? root.feeGrowth / (1 + root.feeGrowth) : 0
    readonly property bool hasAccruedFees: root.accruedShare > 0

    // Apply the (approximate) share to the exact reserve strings, so the printed
    // amount keeps the magnitude of a u128 reserve instead of a double's 15 digits.
    function accruedAmount(reserveRaw) {
        if (!root.hasAccruedFees)
            return "0"
        var scaledShare = String(Math.round(root.accruedShare * 1e12))
        return AmountMath.mulDivFloor(reserveRaw, scaledShare, "1000000000000")
    }

    readonly property string accruedFeesA: root.accruedAmount(root.reserveA)
    readonly property string accruedFeesB: root.accruedAmount(root.reserveB)

    // Spot price from the reserves, computed on the exact decimal strings.
    readonly property string priceAInB: root.hasLiquidity
        ? AmountMath.formatRatio(root.reserveB, root.reserveA, 8) : ""
    readonly property string priceBInA: root.hasLiquidity
        ? AmountMath.formatRatio(root.reserveA, root.reserveB, 8) : ""

    // ── Formatting helpers ───────────────────────────────────────────────────
    function feeLabel(feeBps) {
        var percentage = Number(feeBps) / 100
        return qsTr("%1%").arg(percentage.toLocaleString(Qt.locale(), "f", 2))
    }

    // Group an exact decimal string without routing it through Number(), which
    // would round any reserve past ~15 digits.
    function amountText(rawValue) {
        var digits = String(rawValue).replace(/[^0-9]/g, "").replace(/^0+(?=[0-9])/, "")
        if (digits.length === 0)
            return "0"
        var separator = Qt.locale().groupSeparator
        var grouped = ""
        for (var i = 0; i < digits.length; ++i) {
            if (i > 0 && (digits.length - i) % 3 === 0)
                grouped += separator
            grouped += digits[i]
        }
        return grouped
    }

    function percentText(share) {
        return qsTr("%1%").arg((share * 100).toLocaleString(Qt.locale(), "f", 1))
    }

    // Fee growth needs far more resolution than a balance share: one swap of 1% of
    // the reserve moves a 1bps pool by ~0.00005%, which rounds away at 1 decimal.
    function growthText(share) {
        return qsTr("%1%").arg((share * 100).toLocaleString(Qt.locale(), "f", 4))
    }

    function shortId(value) {
        var text = String(value || "")
        return text.length > 0 ? text : qsTr("—")
    }

    function openRemoveDialog() {
        removeDialog.openFor({
            "symbolA": root.symbolA,
            "symbolB": root.symbolB,
            "tokenAId": root.definitionIdA,
            "tokenBId": root.definitionIdB,
            "lpBalance": root.lpHoldingBalance,
            "lpBalanceTotal": root.lpBalanceTotal,
            "lpHoldingId": root.lpHoldingId,
            // Every LP holding for this pool, so the sheet can offer the choice of
            // which to burn from (encoding-tolerant match, same as applyHoldings).
            "lpHoldings": root.holdingsFor(root.lpDefinitionId),
            "holdingAId": root.holdingAId,
            "holdingBId": root.holdingBId
        })
    }

    AmmTheme {
        id: theme
    }

    Rectangle {
        anchors.fill: parent
        color: theme.colors.background
    }

    RemoveLiquidityDialog {
        id: removeDialog

        theme: theme
        backend: root.backend
        runtime: root.runtime

        // The withdrawal moved the reserves and the wallet's LP, so both reads this
        // page shows are stale the moment it lands.
        onRemoved: function(transactionId) {
            removeToast.show(qsTr("Liquidity removed"), transactionId)
            root.refresh()
        }
    }

    SuccessToast {
        id: removeToast

        objectName: "poolDetailRemoveToast"
        width: Math.max(0, Math.min(380, parent.width - 32))

        anchors {
            bottom: parent.bottom
            bottomMargin: 24
            horizontalCenter: parent.horizontalCenter
        }
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: Math.max(height, pageContent.y + pageContent.implicitHeight
                                + root.pageMargin)
        flickableDirection: Flickable.VerticalFlick
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
        }

        Item {
            id: pageContent

            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.width < 640 ? 24 : 40
            width: Math.max(0, Math.min(root.contentMaxWidth,
                                        scroll.width - root.pageMargin * 2))
            implicitHeight: contentColumn.implicitHeight

            ColumnLayout {
                id: contentColumn

                width: parent.width
                spacing: 20

                // ── Back link ────────────────────────────────────────────────
                Item {
                    id: backLink

                    objectName: "poolDetailBackLink"
                    Layout.preferredWidth: backText.implicitWidth + 8
                    Layout.preferredHeight: 24
                    activeFocusOnTab: true

                    Accessible.role: Accessible.Button
                    Accessible.name: backText.text
                    Accessible.onPressAction: backLink.activate()

                    function activate() {
                        root.backRequested()
                    }

                    Text {
                        id: backText

                        anchors.verticalCenter: parent.verticalCenter
                        text: qsTr("← Pools")
                        color: backMouse.containsMouse || backLink.activeFocus
                               ? theme.colors.textPrimary : theme.colors.textSecondary
                        font.pixelSize: 13
                        font.weight: Font.Medium
                    }

                    MouseArea {
                        id: backMouse

                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: backLink.activate()
                    }

                    Keys.onReturnPressed: backLink.activate()
                    Keys.onEnterPressed: backLink.activate()
                    Keys.onSpacePressed: backLink.activate()
                }

                // ── Header: pair, fee tier, actions ──────────────────────────
                GridLayout {
                    Layout.fillWidth: true
                    columns: root.wideLayout ? 2 : 1
                    columnSpacing: 24
                    rowSpacing: 16

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 6

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 12

                            Item {
                                Layout.preferredWidth: 58
                                Layout.preferredHeight: 36
                                Accessible.ignored: true

                                TokenAvatar {
                                    x: 0
                                    y: 0
                                    symbol: root.symbolA
                                    z: 1
                                }

                                TokenAvatar {
                                    x: 22
                                    y: 0
                                    symbol: root.symbolB
                                }
                            }

                            Text {
                                objectName: "poolDetailPairText"
                                text: root.pairText
                                color: theme.colors.textPrimary
                                font.pixelSize: 30
                                font.weight: Font.Bold
                                elide: Text.ElideRight
                                Layout.fillWidth: true
                            }

                            Rectangle {
                                Layout.preferredWidth: headerFeeText.implicitWidth + 20
                                Layout.preferredHeight: 30
                                radius: 6
                                color: theme.colors.inputBg
                                border.color: theme.colors.borderStrong
                                border.width: 1

                                Text {
                                    id: headerFeeText

                                    objectName: "poolDetailFeeText"
                                    anchors.centerIn: parent
                                    text: root.feeLabel(root.feeBps)
                                    color: theme.colors.textPrimary
                                    font.pixelSize: 13
                                    font.weight: Font.Medium
                                }
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            text: root.poolId.length > 0
                                  ? qsTr("Pool %1").arg(root.poolId)
                                  : qsTr("Pool address unavailable")
                            color: theme.colors.textSecondary
                            font.pixelSize: 12
                            elide: Text.ElideMiddle
                        }
                    }

                    RowLayout {
                        Layout.fillWidth: !root.wideLayout
                        Layout.alignment: root.wideLayout ? Qt.AlignRight | Qt.AlignVCenter
                                                          : Qt.AlignLeft
                        spacing: 12

                        AmmPrimaryButton {
                            objectName: "poolDetailSwapButton"
                            theme: theme
                            text: qsTr("Swap")
                            enabled: root.canSwap
                            implicitHeight: 44
                            Layout.fillWidth: !root.wideLayout
                            Layout.preferredWidth: root.wideLayout ? 120 : -1
                            onClicked: root.swapRequested(root.pool)
                        }

                        // Plain "Add liquidity" until the wallet actually holds LP
                        // here; then it becomes a manage menu offering both directions.
                        Item {
                            id: manageAction

                            objectName: "poolDetailManageButton"
                            Layout.fillWidth: !root.wideLayout
                            Layout.preferredWidth: root.wideLayout
                                                   ? (root.hasPosition ? 170 : 150) : -1
                            Layout.preferredHeight: 44

                            // Same hover bookkeeping as the nav bar's tab menu: the
                            // pointer is over neither while it crosses the gap.
                            property bool pointerOnButton: false
                            property bool pointerInMenu: false

                            function openMenu() {
                                if (!root.hasPosition)
                                    return
                                manageMenuCloseTimer.stop()
                                manageMenu.open()
                            }

                            function scheduleMenuClose() {
                                manageMenuCloseTimer.restart()
                            }

                            Timer {
                                id: manageMenuCloseTimer

                                interval: 180
                                onTriggered: {
                                    if (!manageAction.pointerOnButton && !manageAction.pointerInMenu)
                                        manageMenu.close()
                                }
                            }

                            AmmSecondaryButton {
                                id: manageButton

                                objectName: "poolDetailAddLiquidityButton"
                                anchors.fill: parent
                                theme: theme
                                text: root.hasPosition ? qsTr("Manage position")
                                                       : qsTr("Add liquidity")
                                enabled: root.canAddLiquidity

                                onHoveredChanged: {
                                    manageAction.pointerOnButton = hovered
                                    if (hovered)
                                        manageAction.openMenu()
                                    else
                                        manageAction.scheduleMenuClose()
                                }

                                onClicked: {
                                    if (root.hasPosition)
                                        manageAction.openMenu()
                                    else
                                        root.addLiquidityRequested(root.pool)
                                }
                            }

                            Popup {
                                id: manageMenu

                                objectName: "poolDetailManageMenu"
                                y: manageAction.height + 6
                                width: 190
                                padding: 6
                                closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

                                background: Rectangle {
                                    radius: 12
                                    color: theme.colors.cardBg
                                    border.color: theme.colors.borderStrong
                                    border.width: 1
                                }

                                contentItem: Column {
                                    spacing: 2

                                    HoverHandler {
                                        onHoveredChanged: {
                                            manageAction.pointerInMenu = hovered
                                            if (hovered)
                                                manageMenuCloseTimer.stop()
                                            else
                                                manageAction.scheduleMenuClose()
                                        }
                                    }

                                    ManageEntry {
                                        objectName: "poolDetailManageAdd"
                                        width: manageMenu.availableWidth
                                        text: qsTr("Add liquidity")
                                        onActivated: {
                                            manageMenu.close()
                                            root.addLiquidityRequested(root.pool)
                                        }
                                    }

                                    ManageEntry {
                                        objectName: "poolDetailManageRemove"
                                        width: manageMenu.availableWidth
                                        text: qsTr("Remove liquidity")
                                        enabled: root.canRemoveLiquidity
                                        onActivated: {
                                            manageMenu.close()
                                            root.openRemoveDialog()
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // ── Status banner ────────────────────────────────────────────
                Rectangle {
                    id: statusBanner

                    objectName: "poolDetailStatusBanner"
                    readonly property string message: {
                        if (root.poolError.length > 0)
                            return root.poolError
                        if (root.chainLoading || !root.chainResolved)
                            return qsTr("Loading pool state…")
                        if (!root.poolExists)
                            return qsTr("This pool has not been created on-chain yet. Add liquidity to create it.")
                        if (!root.hasLiquidity)
                            return qsTr("This pool holds no liquidity.")
                        return ""
                    }
                    readonly property bool isError: root.poolError.length > 0

                    Layout.fillWidth: true
                    visible: message.length > 0
                    implicitHeight: visible ? statusText.implicitHeight + 28 : 0
                    radius: 12
                    color: theme.colors.cardBg
                    border.color: isError ? theme.colors.error : theme.colors.border
                    border.width: 1

                    Text {
                        id: statusText

                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.leftMargin: 16
                        anchors.rightMargin: 16
                        anchors.verticalCenter: parent.verticalCenter
                        text: statusBanner.message
                        color: statusBanner.isError ? theme.colors.error : theme.colors.textSecondary
                        font.pixelSize: 13
                        wrapMode: Text.Wrap
                    }
                }

                // ── Body: balances + price | stats + fees + addresses ────────
                GridLayout {
                    Layout.fillWidth: true
                    columns: root.wideLayout ? 2 : 1
                    columnSpacing: 20
                    rowSpacing: 20

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignTop
                        spacing: 20

                        // ── Pool balances ────────────────────────────────────
                        AmmDetailCard {
                            objectName: "poolBalancesCard"
                            theme: theme
                            title: qsTr("Pool balances")
                            contentSpacing: 14
                            Layout.fillWidth: true

                            // Stacked bar: each side's share of the pool's raw units.
                            Rectangle {
                                id: balanceBar

                                objectName: "poolBalanceBar"
                                Layout.fillWidth: true
                                Layout.preferredHeight: 10
                                radius: 5
                                color: theme.colors.inputBg
                                clip: true
                                Accessible.ignored: true

                                readonly property real shareA: root.hasLiquidity ? root.balanceShareA : 0.5

                                Rectangle {
                                    anchors.left: parent.left
                                    anchors.top: parent.top
                                    anchors.bottom: parent.bottom
                                    width: parent.width * balanceBar.shareA
                                    color: root.hasLiquidity
                                           ? TokenVisuals.colorFor(root.symbolA)
                                           : theme.colors.noTokenCircle

                                    Behavior on width {
                                        NumberAnimation { duration: 180; easing.type: Easing.OutCubic }
                                    }
                                }

                                Rectangle {
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    anchors.bottom: parent.bottom
                                    width: parent.width * (1 - balanceBar.shareA)
                                    color: root.hasLiquidity
                                           ? TokenVisuals.colorFor(root.symbolB)
                                           : theme.colors.noTokenCircle

                                    Behavior on width {
                                        NumberAnimation { duration: 180; easing.type: Easing.OutCubic }
                                    }
                                }
                            }

                            BalanceRow {
                                objectName: "poolBalanceRowA"
                                symbol: root.symbolA
                                amount: root.hasLiquidity ? root.amountText(root.reserveA) : qsTr("—")
                                share: root.hasLiquidity ? root.percentText(root.balanceShareA) : ""
                            }

                            BalanceRow {
                                objectName: "poolBalanceRowB"
                                symbol: root.symbolB
                                amount: root.hasLiquidity ? root.amountText(root.reserveB) : qsTr("—")
                                share: root.hasLiquidity ? root.percentText(root.balanceShareB) : ""
                            }

                            Text {
                                Layout.fillWidth: true
                                text: qsTr("Split by token units held, which reflects the pool's own price for the pair.")
                                color: theme.colors.textPlaceholder
                                font.pixelSize: 11
                                wrapMode: Text.Wrap
                            }
                        }

                        // ── Price ────────────────────────────────────────────
                        AmmDetailCard {
                            objectName: "poolPriceCard"
                            theme: theme
                            title: qsTr("Price")
                            Layout.fillWidth: true

                            StatRow {
                                objectName: "poolPriceRowA"
                                label: qsTr("1 %1").arg(root.symbolA)
                                value: root.priceAInB.length > 0
                                       ? qsTr("%1 %2").arg(root.priceAInB).arg(root.symbolB)
                                       : qsTr("—")
                            }

                            StatRow {
                                objectName: "poolPriceRowB"
                                label: qsTr("1 %1").arg(root.symbolB)
                                value: root.priceBInA.length > 0
                                       ? qsTr("%1 %2").arg(root.priceBInA).arg(root.symbolA)
                                       : qsTr("—")
                            }

                            Text {
                                Layout.fillWidth: true
                                text: qsTr("Spot price from the current reserves, before fees and price impact.")
                                color: theme.colors.textPlaceholder
                                font.pixelSize: 11
                                wrapMode: Text.Wrap
                            }
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.preferredWidth: root.wideLayout ? 320 : -1
                        Layout.maximumWidth: root.wideLayout ? 320 : Number.POSITIVE_INFINITY
                        Layout.alignment: Qt.AlignTop
                        spacing: 20

                        // ── Stats ────────────────────────────────────────────
                        AmmDetailCard {
                            objectName: "poolStatsCard"
                            theme: theme
                            title: qsTr("Stats")
                            Layout.fillWidth: true

                            StatRow {
                                objectName: "poolStatFeeTier"
                                label: qsTr("Fee tier")
                                value: root.feeLabel(root.feeBps)
                            }

                            StatRow {
                                objectName: "poolStatReserveA"
                                label: qsTr("%1 reserve").arg(root.symbolA)
                                value: root.poolExists ? root.amountText(root.reserveA) : qsTr("—")
                            }

                            StatRow {
                                objectName: "poolStatReserveB"
                                label: qsTr("%1 reserve").arg(root.symbolB)
                                value: root.poolExists ? root.amountText(root.reserveB) : qsTr("—")
                            }

                            StatRow {
                                objectName: "poolStatLpSupply"
                                label: qsTr("LP token supply")
                                value: root.poolExists ? root.amountText(root.liquiditySupply) : qsTr("—")
                            }
                        }

                        // ── Accumulated fees ─────────────────────────────────
                        AmmDetailCard {
                            objectName: "poolFeesCard"
                            theme: theme
                            title: qsTr("Accumulated fees")
                            Layout.fillWidth: true

                            StatRow {
                                objectName: "poolFeesRowA"
                                label: root.symbolA
                                value: root.hasAccruedFees ? root.amountText(root.accruedFeesA) : qsTr("—")
                            }

                            StatRow {
                                objectName: "poolFeesRowB"
                                label: root.symbolB
                                value: root.hasAccruedFees ? root.amountText(root.accruedFeesB) : qsTr("—")
                            }

                            StatRow {
                                objectName: "poolFeesGrowthRow"
                                label: qsTr("Growth since creation")
                                value: root.hasAccruedFees
                                       ? root.growthText(root.feeGrowth) : qsTr("—")
                            }

                            Text {
                                Layout.fillWidth: true
                                text: root.hasAccruedFees
                                      ? qsTr("Estimated. Swap fees stay in the reserves instead of a counter, so this is derived from how far the reserves have grown ahead of the LP supply.")
                                      : qsTr("Swap fees stay in the reserves instead of a counter. Nothing has accrued beyond the deposits yet.")
                                color: theme.colors.textPlaceholder
                                font.pixelSize: 11
                                wrapMode: Text.Wrap
                            }
                        }

                        // ── Addresses ────────────────────────────────────────
                        AmmDetailCard {
                            objectName: "poolAddressesCard"
                            theme: theme
                            title: qsTr("Addresses")
                            Layout.fillWidth: true

                            AddressRow {
                                objectName: "poolAddressPool"
                                label: qsTr("Pool")
                                value: root.shortId(root.poolId)
                            }

                            AddressRow {
                                objectName: "poolAddressVaultA"
                                label: qsTr("%1 vault").arg(root.symbolA)
                                value: root.shortId(root.vaultAId)
                            }

                            AddressRow {
                                objectName: "poolAddressVaultB"
                                label: qsTr("%1 vault").arg(root.symbolB)
                                value: root.shortId(root.vaultBId)
                            }

                            AddressRow {
                                objectName: "poolAddressLpToken"
                                label: qsTr("LP token")
                                value: root.shortId(root.lpDefinitionId)
                            }
                        }
                    }
                }
            }
        }
    }

    // ── Page-local building blocks ───────────────────────────────────────────

    // Anchors rather than a RowLayout: the value is capped against the row's own
    // width, and a layout child whose maximumWidth depends on the row width can
    // feed back into the row's implicit size.
    component ManageEntry: Rectangle {
        id: entry

        property string text: ""
        signal activated()

        height: 36
        radius: 8
        color: entryMouse.containsMouse && entry.enabled
               ? theme.colors.panelHoverBg : "transparent"

        Accessible.role: Accessible.MenuItem
        Accessible.name: entry.text

        Text {
            anchors.left: parent.left
            anchors.leftMargin: 10
            anchors.right: parent.right
            anchors.rightMargin: 10
            anchors.verticalCenter: parent.verticalCenter
            text: entry.text
            color: entry.enabled ? theme.colors.textPrimary : theme.colors.textPlaceholder
            font.pixelSize: 14
            elide: Text.ElideRight
        }

        MouseArea {
            id: entryMouse

            anchors.fill: parent
            hoverEnabled: true
            cursorShape: entry.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
            onClicked: {
                if (entry.enabled)
                    entry.activated()
            }
        }
    }

    component StatRow: Item {
        id: statRow

        property string label: ""
        property string value: ""

        Layout.fillWidth: true
        implicitHeight: Math.max(statLabel.implicitHeight, statValue.implicitHeight)

        Text {
            id: statLabel

            anchors.left: parent.left
            anchors.right: statValue.left
            anchors.rightMargin: 12
            anchors.verticalCenter: parent.verticalCenter
            text: statRow.label
            color: theme.colors.textSecondary
            font.pixelSize: 13
            elide: Text.ElideRight
        }

        Text {
            id: statValue

            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, statRow.width * 0.7)
            text: statRow.value
            color: theme.colors.textPrimary
            font.pixelSize: 13
            font.weight: Font.Medium
            horizontalAlignment: Text.AlignRight
            elide: Text.ElideRight
        }
    }

    component AddressRow: ColumnLayout {
        id: addressRow

        property string label: ""
        property string value: ""

        Layout.fillWidth: true
        spacing: 2

        Text {
            text: addressRow.label
            color: theme.colors.textSecondary
            font.pixelSize: 12
            elide: Text.ElideRight
            Layout.fillWidth: true
        }

        Text {
            text: addressRow.value
            color: theme.colors.textPrimary
            font.pixelSize: 12
            font.family: "monospace"
            elide: Text.ElideMiddle
            Layout.fillWidth: true
        }
    }

    component BalanceRow: RowLayout {
        id: balanceRow

        property string symbol: ""
        property string amount: ""
        property string share: ""

        Layout.fillWidth: true
        spacing: 10

        Rectangle {
            Layout.preferredWidth: 10
            Layout.preferredHeight: 10
            radius: 5
            color: TokenVisuals.colorFor(balanceRow.symbol)
            Accessible.ignored: true
        }

        // Both sides absorb the shortfall (and both elide), so a long raw reserve
        // can't push the percentage out of the card.
        Text {
            text: balanceRow.symbol
            color: theme.colors.textPrimary
            font.pixelSize: 14
            font.weight: Font.Medium
            elide: Text.ElideRight
            Layout.fillWidth: true
            Layout.preferredWidth: implicitWidth
        }

        Text {
            text: balanceRow.amount
            color: theme.colors.textPrimary
            font.pixelSize: 14
            horizontalAlignment: Text.AlignRight
            elide: Text.ElideRight
            Layout.fillWidth: true
            Layout.preferredWidth: implicitWidth
        }

        Text {
            visible: balanceRow.share.length > 0
            text: balanceRow.share
            color: theme.colors.textSecondary
            font.pixelSize: 12
            horizontalAlignment: Text.AlignRight
            Layout.preferredWidth: 46
        }
    }

    component TokenAvatar: Rectangle {
        id: avatar

        required property string symbol

        width: 36
        height: 36
        radius: 18
        color: TokenVisuals.colorFor(symbol)
        border.color: theme.colors.cardBg
        border.width: 2
        Accessible.ignored: true

        Text {
            anchors.centerIn: parent
            text: TokenVisuals.letterFor(avatar.symbol)
            color: "#FFFFFF"
            font.pixelSize: 13
            font.weight: Font.Bold
            Accessible.ignored: true
        }
    }
}
