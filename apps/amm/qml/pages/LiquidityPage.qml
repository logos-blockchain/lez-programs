import QtQuick 2.15
import "../components/shared"
import "../components/liquidity"
import "../state"

Item {
    id: root

    readonly property int pageMargin: 16
    readonly property int preferredCardWidth: 960
    readonly property int pageCardY: newPositionForm.implicitHeight + root.pageMargin * 2 <= scroll.height ? Math.max(root.pageMargin, Math.round((scroll.height - newPositionForm.implicitHeight) / 4)) : root.pageMargin

    width: parent ? parent.width : implicitWidth
    height: parent ? parent.height : implicitHeight
    implicitWidth: root.preferredCardWidth + root.pageMargin * 2
    implicitHeight: newPositionForm.implicitHeight + root.pageMargin * 2

    NewPositionPrototypeBackend {
        id: newPositionBackend
    }

    Rectangle {
        anchors.fill: parent
        color: "#151515"
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentHeight: Math.max(height, newPositionForm.y + newPositionForm.implicitHeight + root.pageMargin)
        contentWidth: width
        enabled: !confirmationDialog.open
        flickableDirection: Flickable.VerticalFlick

        NewPositionForm {
            id: newPositionForm

            prototypeBackend: newPositionBackend
            width: Math.max(0, Math.min(scroll.width - root.pageMargin * 2, root.preferredCardWidth))
            x: Math.max(root.pageMargin, (scroll.width - width) / 2)
            y: root.pageCardY

            onConfirmationRequested: function (snapshot) {
                confirmationDialog.openWithSnapshot(snapshot);
            }
        }
    }

    SuccessToast {
        id: successToast

        width: Math.max(0, Math.min(380, root.width - 32))
        z: 30

        anchors {
            bottom: parent.bottom
            bottomMargin: 18
            horizontalCenter: parent.horizontalCenter
        }
    }

    NewPositionConfirmationDialog {
        id: confirmationDialog

        anchors.fill: parent

        onConfirmed: function (snapshot) {
            root.confirmNewPosition(snapshot);
        }
    }

    function confirmNewPosition(snapshot) {
        const result = newPositionBackend.submitNewPosition(snapshot.request, snapshot.quoteHash);

        if (result.status !== "ok") {
            newPositionForm.setSubmitError(result.error);
            return;
        }

        newPositionForm.resetAfterSubmit();
        successToast.show(result.message, result.detail);
    }
}
