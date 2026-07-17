pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ColumnLayout {
    id: root

    property var snapshot: ({})

    signal snapshotEdited(var snapshot)

    spacing: 8

    function actionText(instruction) {
        if (instruction === "NewDefinition")
            return qsTr("Create pool")
        if (instruction === "AddLiquidity")
            return qsTr("Add liquidity")
        return instruction || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Pair")
        value: root.snapshot.pairText || "-"
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 5
        visible: root.snapshot.instruction === "AddLiquidity"

        Text {
            text: qsTr("LP destination")
            color: "#a1a1aa"
            font.pixelSize: 12
        }

        ComboBox {
            id: lpDestinationPicker

            objectName: "lpDestinationPicker"
            Layout.fillWidth: true
            model: root.destinationRows()
            enabled: model.length > 1
            currentIndex: root.destinationIndex()
            displayText: currentIndex >= 0
                         ? model[currentIndex].label : qsTr("Select destination")
            Accessible.name: qsTr("LP token destination")
            onActivated: function(index) {
                root.selectDestination(model[index])
            }

            delegate: ItemDelegate {
                required property var modelData
                width: lpDestinationPicker.width
                text: modelData.label
            }
        }
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Action")
        value: root.actionText(root.snapshot.instruction)
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Fee")
        value: root.snapshot.feeText || "-"
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Deposit")
        value: qsTr("%1 + %2")
            .arg(root.snapshot.depositAText || "-")
            .arg(root.snapshot.depositBText || "-")
        valueWrapAnywhere: true
    }

    SummaryRow {
        Layout.fillWidth: true
        label: qsTr("Expected LP")
        value: root.snapshot.expectedLpText || "-"
    }

    function destinationRows() {
        var rows = []
        var options = root.snapshot.lpHoldingOptions || []
        for (var i = 0; i < options.length; ++i) {
            var id = String(options[i].holdingId || "")
            rows.push({
                "holdingId": id,
                "createFresh": false,
                "label": qsTr("%1 · balance %2")
                    .arg(root.shortId(id))
                    .arg(String(options[i].balanceRaw || "0"))
            })
        }
        rows.push({
            "holdingId": "",
            "createFresh": true,
            "label": qsTr("Create new LP holding")
        })
        return rows
    }

    function destinationIndex() {
        var rows = root.destinationRows()
        for (var i = 0; i < rows.length; ++i) {
            if (rows[i].createFresh === (root.snapshot.createFreshLp === true)
                    && (rows[i].createFresh
                        || rows[i].holdingId === root.snapshot.selectedLpHoldingId)) {
                return i
            }
        }
        return -1
    }

    function selectDestination(destination) {
        var next = JSON.parse(JSON.stringify(root.snapshot || ({})))
        next.request = next.request || ({})
        delete next.request.lpHoldingId
        next.request.createFreshLp = destination.createFresh === true
        if (!destination.createFresh)
            next.request.lpHoldingId = destination.holdingId
        next.selectedLpHoldingId = destination.holdingId
        next.createFreshLp = destination.createFresh === true
        next.lpDestinationRequired = false
        next.quoteReady = false
        root.snapshotEdited(next)
    }

    function shortId(value) {
        return value.length > 14 ? value.slice(0, 7) + "…" + value.slice(-5) : value
    }
}
