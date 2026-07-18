import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Popup {
    id: root

    property string title: qsTr("Confirm transaction")
    property string cancelText: qsTr("Cancel")
    property string confirmText: qsTr("Confirm")
    property bool busy: false
    property string busyText: qsTr("Submitting…")
    property bool activityBusy: false
    property string activityText: qsTr("Updating…")
    property bool showInlineBusyIndicator: true
    property var snapshot: ({})
    property Component summary: null
    property bool confirmationPending: false
    property bool confirmEnabled: true
    property bool roundedCancelButton: false
    property bool closeWhenSettled: true
    readonly property bool actionPending: root.busy || root.activityBusy

    signal canceled
    signal confirmed(var snapshot)
    signal summaryEdited(var snapshot)

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
        Qt.callLater(function() {
            if (cancelButtonLoader.item)
                cancelButtonLoader.item.forceActiveFocus()
        })
    }

    function updateSnapshot(nextSnapshot) {
        root.snapshot = root.cloneSnapshot(nextSnapshot)
    }

    function cancel() {
        if (root.busy)
            return
        root.confirmationPending = false
        root.close()
        root.canceled()
    }

    function confirm() {
        if (root.actionPending || !root.confirmEnabled)
            return
        root.confirmationPending = true
        root.confirmed(root.snapshot)
        if (!root.busy) {
            root.confirmationPending = false
            root.close()
        }
    }

    Connections {
        target: summaryLoader.item
        ignoreUnknownSignals: true

        function onSnapshotEdited(snapshot) {
            root.updateSnapshot(snapshot)
            root.summaryEdited(root.snapshot)
        }
    }

    onBusyChanged: {
        if (!root.busy && root.confirmationPending) {
            root.confirmationPending = false
            if (root.closeWhenSettled)
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

        Item {
            id: inlineBusyIndicator

            property bool active: root.showInlineBusyIndicator && root.actionPending
            Layout.alignment: Qt.AlignHCenter
            Layout.preferredWidth: active ? busySpinner.implicitWidth : 0
            Layout.preferredHeight: active ? busySpinner.implicitHeight : 0
            implicitWidth: busySpinner.implicitWidth
            implicitHeight: busySpinner.implicitHeight
            visible: active

            BusyIndicator {
                id: busySpinner

                anchors.centerIn: parent
                running: inlineBusyIndicator.active
                Accessible.name: root.busy ? root.busyText : root.activityText
            }
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: 10

            Loader {
                id: cancelButtonLoader
                objectName: "transactionCancelButtonLoader"
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                sourceComponent: root.roundedCancelButton
                                 ? roundedCancelButtonComponent : defaultCancelButtonComponent
            }

            Button {
                id: confirmButton
                objectName: "transactionConfirmButton"
                Layout.fillWidth: true
                implicitHeight: 44
                text: root.busy ? root.busyText : root.confirmText
                enabled: !root.actionPending && root.confirmEnabled
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

    Component {
        id: defaultCancelButtonComponent

        Button {
            objectName: "transactionCancelButton"
            anchors.fill: parent
            text: root.cancelText
            enabled: !root.busy
            Accessible.name: text
            onClicked: root.cancel()
        }
    }

    Component {
        id: roundedCancelButtonComponent

        Button {
            id: cancelButton

            objectName: "transactionCancelButton"
            anchors.fill: parent
            text: root.cancelText
            enabled: !root.busy
            Accessible.name: text
            onClicked: root.cancel()

            background: Rectangle {
                color: cancelButton.pressed ? "#3f3f46"
                      : cancelButton.hovered ? "#27272a" : "#18181b"
                border.color: "#52525b"
                border.width: 1
                radius: 6
            }

            contentItem: Label {
                text: cancelButton.text
                color: "#f4f4f5"
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
            }
        }
    }
}
