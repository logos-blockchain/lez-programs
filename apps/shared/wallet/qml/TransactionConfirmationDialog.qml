import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Popup {
    id: root

    property string title: qsTr("Confirm transaction")
    property string cancelText: qsTr("Cancel")
    property string confirmText: qsTr("Confirm")
    property bool busy: false
    property var snapshot: ({})
    property Component summary: null
    property bool confirmationPending: false

    signal canceled
    signal confirmed(var snapshot)

    modal: true
    dim: true
    padding: 20
    width: Math.max(0, Math.min(420, parent ? parent.width - 32 : 420))
    height: Math.max(0, Math.min(implicitHeight, parent ? parent.height - 32 : implicitHeight))
    x: parent ? Math.max(0, Math.round((parent.width - width) / 2)) : 0
    y: parent ? Math.max(0, Math.round((parent.height - height) / 2)) : 0
    closePolicy: root.busy ? Popup.NoAutoClose : Popup.CloseOnEscape | Popup.CloseOnPressOutside
    focus: true

    function cloneSnapshot(value) {
        if (value === undefined || value === null)
            return ({})
        try {
            return JSON.parse(JSON.stringify(value))
        } catch (_error) {
            return value
        }
    }

    function openWithSnapshot(nextSnapshot) {
        root.snapshot = root.cloneSnapshot(nextSnapshot)
        root.confirmationPending = false
        root.open()
        cancelButton.forceActiveFocus()
    }

    function cancel() {
        if (root.busy)
            return
        root.confirmationPending = false
        root.close()
        root.canceled()
    }

    function confirm() {
        if (root.busy)
            return
        root.confirmationPending = true
        root.confirmed(root.snapshot)
        if (!root.busy) {
            root.confirmationPending = false
            root.close()
        }
    }

    onBusyChanged: {
        if (!root.busy && root.confirmationPending) {
            root.confirmationPending = false
            root.close()
        }
    }

    onSnapshotChanged: {
        if (summaryLoader.item && summaryLoader.item.hasOwnProperty("snapshot"))
            summaryLoader.item.snapshot = root.snapshot
    }

    Overlay.modal: Rectangle { color: "#99000000" }

    background: Rectangle {
        color: "#18181b"
        border.color: "#3f3f46"
        border.width: 1
        radius: 8
    }

    contentItem: ColumnLayout {
        spacing: 16

        Label {
            Layout.fillWidth: true
            text: root.title
            color: "#f4f4f5"
            font.bold: true
            font.pixelSize: 17
            wrapMode: Text.WordWrap
        }

        ScrollView {
            id: summaryScroll
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.minimumHeight: 0
            Layout.preferredHeight: summaryLoader.implicitHeight
            clip: true
            contentWidth: availableWidth
            ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

            Loader {
                id: summaryLoader
                objectName: "transactionSummaryLoader"
                width: summaryScroll.availableWidth
                sourceComponent: root.summary
                onLoaded: {
                    if (item && item.hasOwnProperty("snapshot"))
                        item.snapshot = root.snapshot
                }
            }
        }

        BusyIndicator {
            Layout.alignment: Qt.AlignHCenter
            visible: root.busy
            running: root.busy
            Accessible.name: qsTr("Submitting transaction")
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10

            Button {
                id: cancelButton
                objectName: "transactionCancelButton"
                Layout.fillWidth: true
                implicitHeight: 44
                text: root.cancelText
                enabled: !root.busy
                Accessible.name: text
                onClicked: root.cancel()
            }

            Button {
                id: confirmButton
                objectName: "transactionConfirmButton"
                Layout.fillWidth: true
                implicitHeight: 44
                text: root.busy ? qsTr("Submitting...") : root.confirmText
                enabled: !root.busy
                Accessible.name: text
                onClicked: root.confirm()

                background: Rectangle {
                    color: confirmButton.enabled
                        ? confirmButton.pressed ? "#d95c1e" : "#f26a21"
                        : "#52525b"
                    radius: 6
                }

                contentItem: Label {
                    text: confirmButton.text
                    color: "#ffffff"
                    font.bold: true
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
            }
        }
    }
}
