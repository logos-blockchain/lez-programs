import QtQuick 2.15
import QtQuick.Layouts 1.15
import "../state"

Rectangle {
    id: root

    required property DummyPoolState poolState

    property real slippageTolerancePercent: 0.5
    property string amountA: ""
    property string amountB: ""
    property string lastEditedToken: "A"
    readonly property real parsedA: root.poolState.parseAmount(root.amountA)
    readonly property real parsedB: root.poolState.parseAmount(root.amountB)
    readonly property var preview: root.poolState.addLiquidityPreview(root.parsedA, root.parsedB)
    readonly property int minLpReceived: root.poolState.minReceivedAmount(root.preview.deltaLp, root.slippageTolerancePercent)
    readonly property bool hasAnyAmount: root.parsedA > 0 || root.parsedB > 0
    readonly property bool amountAOverBalance: root.parsedA > root.poolState.walletBalanceA
    readonly property bool amountBOverBalance: root.parsedB > root.poolState.walletBalanceB
    readonly property bool minReceivedIsZero: root.hasAnyAmount && root.minLpReceived === 0
    readonly property bool zeroTokenDeposit: root.hasAnyAmount && (root.preview.actualA === 0 || root.preview.actualB === 0)
    readonly property bool zeroLpDeposit: root.preview.actualA > 0 && root.preview.actualB > 0 && root.preview.deltaLp === 0
    readonly property string warningText: root.zeroTokenDeposit ? qsTr("Deposit would be rejected because one token amount rounds to zero") : root.zeroLpDeposit ? qsTr("Deposit would mint 0 LP tokens") : ""

    signal slippageToleranceChangeRequested(real tolerancePercent)

    color: "#1D1D1D"
    implicitHeight: content.implicitHeight + 20
    radius: 8
    border.color: "#343434"
    border.width: 1

    function setAmounts(nextA, nextB, intentToken, showZero) {
        root.lastEditedToken = intentToken;
        root.amountA = nextA > 0 || showZero ? root.poolState.formatInputAmount(nextA) : "";
        root.amountB = nextB > 0 || showZero ? root.poolState.formatInputAmount(nextB) : "";
    }

    function updateFromTokenA(value) {
        if (value.length === 0) {
            setAmounts(0, 0, "A", false);
            return;
        }

        const nextA = root.poolState.parseAmount(value);
        setAmounts(nextA, root.poolState.amountBForA(nextA), "A", true);
    }

    function updateFromTokenB(value) {
        if (value.length === 0) {
            setAmounts(0, 0, "B", false);
            return;
        }

        const nextB = root.poolState.parseAmount(value);
        setAmounts(root.poolState.amountAForB(nextB), nextB, "B", true);
    }

    function useMax(intentToken) {
        const capped = root.poolState.maxAddLiquidityForBalances();
        setAmounts(capped.actualA, capped.actualB, intentToken, false);
    }

    ColumnLayout {
        id: content

        anchors.fill: parent
        anchors.margins: 10
        spacing: 10

        Text {
            color: "#E7E1D8"
            font.bold: true
            font.pixelSize: 16
            text: qsTr("Add liquidity")

            Layout.fillWidth: true
        }

        SummaryRow {
            label: qsTr("Current ratio")
            value: qsTr("1 %1 = %2 %3").arg(root.poolState.tokenB).arg(root.poolState.formatInteger(root.poolState.tokenAPerTokenB)).arg(root.poolState.tokenA)

            Layout.fillWidth: true
        }

        TokenAmountInput {
            balance: root.poolState.formatTokenAmount(root.poolState.walletBalanceA, root.poolState.tokenA)
            errorText: root.amountAOverBalance ? qsTr("Insufficient %1 balance").arg(root.poolState.tokenA) : ""
            helperText: root.lastEditedToken === "B" && root.amountA.length > 0 ? qsTr("Calculated from current pool ratio") : ""
            label: qsTr("Token A amount")
            token: root.poolState.tokenA
            text: root.amountA

            Layout.fillWidth: true

            onEditingChanged: function (value) {
                root.updateFromTokenA(value);
            }
            onMaxClicked: root.useMax("A")
        }

        TokenAmountInput {
            balance: root.poolState.formatTokenAmount(root.poolState.walletBalanceB, root.poolState.tokenB)
            errorText: root.amountBOverBalance ? qsTr("Insufficient %1 balance").arg(root.poolState.tokenB) : ""
            helperText: root.lastEditedToken === "A" && root.amountB.length > 0 ? qsTr("Calculated from current pool ratio") : ""
            label: qsTr("Token B amount")
            token: root.poolState.tokenB
            text: root.amountB

            Layout.fillWidth: true

            onEditingChanged: function (value) {
                root.updateFromTokenB(value);
            }
            onMaxClicked: root.useMax("B")
        }

        SummaryRow {
            label: qsTr("Required ratio")
            value: qsTr("%1 %2 / 1 %3").arg(root.poolState.formatInteger(root.poolState.tokenAPerTokenB)).arg(root.poolState.tokenA).arg(root.poolState.tokenB)

            Layout.fillWidth: true
        }

        SummaryRow {
            estimated: true
            estimateHelp: qsTr("Estimated with the same integer floor math used by the add-liquidity contract path.")
            label: qsTr("Estimated LP tokens")
            value: root.poolState.formatLpAmount(root.preview.deltaLp)

            Layout.fillWidth: true
        }

        SlippageToleranceControl {
            tolerancePercent: root.slippageTolerancePercent

            Layout.fillWidth: true

            onToleranceChangeRequested: function (tolerancePercent) {
                root.slippageToleranceChangeRequested(tolerancePercent);
            }
        }

        SummaryRow {
            label: qsTr("Min LP received")
            value: root.poolState.formatLpAmount(root.minLpReceived)

            Layout.fillWidth: true
        }

        Text {
            color: "#F08A76"
            font.pixelSize: 12
            lineHeight: 1.25
            text: qsTr("Minimum received is 0. Increase amount or lower slippage.")
            visible: root.minReceivedIsZero
            wrapMode: Text.WordWrap

            Layout.fillWidth: true
        }

        Text {
            color: "#F08A76"
            font.pixelSize: 12
            lineHeight: 1.25
            text: root.warningText
            visible: root.warningText.length > 0
            wrapMode: Text.WordWrap

            Layout.fillWidth: true
        }
    }
}
