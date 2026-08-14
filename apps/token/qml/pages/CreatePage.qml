pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Layouts

Item {
    id: root

    property var store: null
    property var backend: null
    property var runtime: null
    property int tokenKind: 0
    property int authorityMode: 0
    property bool metadataEnabled: false
    property int metadataStandard: 0
    property int step: 0
    property bool prepared: false
    property string preparedMessage: ""
    property string errorMessage: ""
    property bool submitting: false
    property bool accountBusy: false

    signal requestInspect()

    onVisibleChanged: {
        if (visible) {
            step = 0;
            prepared = false;
            preparedMessage = "";
            errorMessage = "";
            submitting = false;
            scroll.contentY = 0;
        }
    }

    readonly property bool isFungible: tokenKind === 0
    readonly property bool requiresMetadata: !isFungible
    readonly property bool hasMetadata: requiresMetadata || metadataEnabled
    readonly property string maximumU128: "340282366920938463463374607431768211455"
    readonly property string instructionName: !isFungible ? "createNonFungible" : hasMetadata ? "createFungibleWithMetadata" : "createFungible"
    readonly property string supplyLabel: isFungible ? qsTr("Initial raw supply") : qsTr("Printable supply")
    readonly property string supplyValue: supplyField.text.length > 0 ? supplyField.text : qsTr("Not set")
    readonly property string authorityValue: !isFungible ? qsTr("Master holding controls printing") : authorityMode === 0 ? qsTr("Fixed supply") : authorityMode === 1 ? qsTr("Definition account") : externalAuthorityField.text.length > 0 ? externalAuthorityField.text : qsTr("External account required")
    readonly property bool validSupply: isUnsignedU128(supplyField.text)
    readonly property bool validDefinitionTarget: isAccountId(definitionTargetField.text)
    readonly property bool validHoldingTarget: isAccountId(holdingTargetField.text)
    readonly property bool validMetadataTarget: !hasMetadata || isAccountId(metadataTargetField.text)
    readonly property bool validExternalAuthority: !isFungible || authorityMode !== 2 || (isAccountId(externalAuthorityField.text) && externalAuthorityField.text !== "11111111111111111111111111111111")
    readonly property bool canContinue: validSupply && validExternalAuthority
    readonly property bool canPrepare: canContinue && validDefinitionTarget && validHoldingTarget && validMetadataTarget
    readonly property bool canSubmit: canPrepare && root.backend !== null && root.backend.isWalletOpen && !root.submitting
    readonly property int validTargetCount: (validDefinitionTarget ? 1 : 0) + (validHoldingTarget ? 1 : 0) + (hasMetadata && validMetadataTarget ? 1 : 0)
    readonly property int targetCount: hasMetadata ? 3 : 2

    function isUnsignedU128(value) {
        if (!/^[0-9]+$/.test(value))
            return false;

        var normalized = value.replace(/^0+(?=\d)/, "");
        if (normalized.length < maximumU128.length)
            return true;
        if (normalized.length > maximumU128.length)
            return false;
        return normalized <= maximumU128;
    }

    function isAccountId(value) {
        return /^[0-9a-fA-F]{64}$/.test(value) || /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(value);
    }

    function clearDraftState() {
        prepared = false;
        preparedMessage = "";
        errorMessage = "";
    }

    function watch(result, success, failure) {
        if (root.runtime && root.runtime.watch)
            root.runtime.watch(result, success, failure);
        else if (failure)
            failure(qsTr("Runtime is not ready."));
    }

    function createTargetAccounts() {
        if (!root.backend || !root.backend.isWalletOpen || root.accountBusy)
            return;

        var fields = [definitionTargetField, holdingTargetField];
        if (root.hasMetadata)
            fields.push(metadataTargetField);

        var pendingFields = [];
        for (var index = 0; index < fields.length; ++index) {
            if (!fields[index].text)
                pendingFields.push(fields[index]);
        }

        if (pendingFields.length === 0)
            return;

        root.accountBusy = true;
        function createNext(fieldIndex) {
            if (fieldIndex >= pendingFields.length) {
                root.accountBusy = false;
                root.clearDraftState();
                return;
            }
            root.watch(root.backend.createAccountPublic(), function(accountId) {
                if (!accountId) {
                    root.accountBusy = false;
                    root.errorMessage = qsTr("Could not create a public wallet account.");
                    return;
                }
                pendingFields[fieldIndex].text = String(accountId);
                createNext(fieldIndex + 1);
            }, function(error) {
                root.accountBusy = false;
                root.errorMessage = qsTr("Could not create a public wallet account: %1").arg(error);
            });
        }
        createNext(0);
    }

    function selectTokenKind(kind) {
        tokenKind = kind;
        if (requiresMetadata)
            metadataEnabled = true;
        clearDraftState();
    }

    function setPattern(pattern) {
        clearDraftState();
        step = 0;
        externalAuthorityField.text = "";
        definitionTargetField.text = "";
        holdingTargetField.text = "";
        metadataTargetField.text = "";
        metadataUriField.text = "";
        creatorsField.text = "";

        if (pattern === "fixed") {
            tokenKind = 0;
            authorityMode = 0;
            metadataEnabled = false;
            metadataStandard = 0;
            nameField.text = qsTr("Fixed supply token");
            supplyField.text = "7654321";
            return;
        }

        if (pattern === "metadata") {
            tokenKind = 0;
            authorityMode = 2;
            metadataEnabled = true;
            metadataStandard = 1;
            nameField.text = qsTr("Metadata token");
            supplyField.text = "10000000000000000000000000";
            metadataUriField.text = "data:application/json;base64,...";
            creatorsField.text = qsTr("Creator or authority label");
            return;
        }

        tokenKind = 1;
        authorityMode = 0;
        metadataEnabled = true;
        metadataStandard = 1;
        nameField.text = qsTr("NFT collection");
        supplyField.text = "64";
        metadataUriField.text = "data:application/json;base64,...";
        creatorsField.text = qsTr("Master-holding creator");
    }

    function prepareDefinition() {
        if (!canSubmit)
            return;

        var mintAuthority = !isFungible ? "" : authorityMode === 0 ? "none" : authorityMode === 1 ? "self" : externalAuthorityField.text;
        var metadataStandardValue = metadataStandard === 0 ? "simple" : "expanded";
        var pending;
        root.submitting = true;
        root.errorMessage = "";
        root.preparedMessage = qsTr("Submitting to the Token Program…");

        if (!isFungible && root.backend) {
            pending = root.backend.createNonFungible(
                definitionTargetField.text,
                holdingTargetField.text,
                metadataTargetField.text,
                nameField.text,
                supplyField.text,
                metadataStandardValue,
                metadataUriField.text,
                creatorsField.text);
        } else if (hasMetadata && root.backend) {
            pending = root.backend.createFungibleWithMetadata(
                definitionTargetField.text,
                holdingTargetField.text,
                metadataTargetField.text,
                nameField.text,
                supplyField.text,
                mintAuthority,
                metadataStandardValue,
                metadataUriField.text,
                creatorsField.text);
        } else if (root.backend) {
            pending = root.backend.createFungible(
                definitionTargetField.text,
                holdingTargetField.text,
                nameField.text,
                supplyField.text,
                mintAuthority);
        }

        root.watch(pending, function(result) {
            root.submitting = false;
            if (!result || result.status !== "ok") {
                root.prepared = false;
                root.errorMessage = qsTr("Token Program rejected the request: %1").arg(result && result.error ? result.error : qsTr("unknown_error"));
                return;
            }

            var transactionId = String(result.transactionId || "");
            var definitionType = isFungible ? "fungible" : "nonFungible";
            var draft = {
                id: definitionTargetField.text,
                name: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition"),
                type: definitionType,
                definitionId: definitionTargetField.text,
                holdingId: holdingTargetField.text,
                metadataId: hasMetadata ? metadataTargetField.text : "",
                rawSupply: supplyField.text,
                displaySupply: supplyField.text,
                inferredDecimals: "",
                authorityMode: isFungible ? (authorityMode === 0 ? "fixed" : authorityMode === 1 ? "self" : "external") : "masterHolding",
                authority: isFungible && authorityMode === 2 ? externalAuthorityField.text : authorityMode === 1 ? definitionTargetField.text : "",
                authorityLabel: isFungible && authorityMode === 2 ? qsTr("External authority account") : "",
                metadataStandard: hasMetadata ? (metadataStandard === 0 ? "Simple" : "Expanded") : "",
                metadataUri: hasMetadata ? metadataUriField.text : "",
                creators: hasMetadata ? creatorsField.text : "",
                description: qsTr("Submitted to the Token Program."),
                source: "pending",
                instruction: instructionName,
                printableCopies: !isFungible ? supplyField.text : "",
                masterHolding: !isFungible ? holdingTargetField.text : "",
                transactionId: transactionId,
                definition: {
                    id: definitionTargetField.text,
                    hex: definitionTargetField.text,
                    name: nameField.text,
                    type: definitionType,
                    totalSupplyRaw: isFungible ? supplyField.text : undefined,
                    printableSupply: !isFungible ? supplyField.text : undefined,
                    mintAuthority: isFungible && authorityMode === 2 ? externalAuthorityField.text : isFungible && authorityMode === 1 ? definitionTargetField.text : undefined,
                    metadataId: hasMetadata ? metadataTargetField.text : undefined
                },
                holding: {
                    id: holdingTargetField.text,
                    wallet: "connected wallet",
                    role: !isFungible ? "nftMaster" : "fungible",
                    rawBalance: isFungible ? supplyField.text : undefined,
                    printBalance: !isFungible ? supplyField.text : undefined
                },
                holdings: [{
                    id: holdingTargetField.text,
                    wallet: "connected wallet",
                    role: !isFungible ? "nftMaster" : "fungible",
                    rawBalance: isFungible ? supplyField.text : undefined,
                    printBalance: !isFungible ? supplyField.text : undefined
                }]
            };

            if (hasMetadata) {
                draft.metadata = {
                    id: metadataTargetField.text,
                    standard: metadataStandard === 0 ? "Simple" : "Expanded",
                    uri: metadataUriField.text,
                    creators: creatorsField.text
                };
            }

            if (store && store.addDraft)
                store.addDraft(draft);

            root.prepared = true;
            root.preparedMessage = transactionId.length > 0
                ? qsTr("Transaction submitted · %1").arg(transactionId)
                : qsTr("Transaction submitted.");
        }, function(error) {
            root.submitting = false;
            root.prepared = false;
            root.errorMessage = qsTr("Token Program request failed: %1").arg(error);
        });
    }

    Rectangle {
        anchors.fill: parent
        color: "#151515"
    }

    Menu {
        id: examplesMenu

        MenuItem {
            text: qsTr("Fixed supply")
            onTriggered: root.setPattern("fixed")
        }

        MenuItem {
            text: qsTr("Metadata-backed fungible")
            onTriggered: root.setPattern("metadata")
        }

        MenuItem {
            text: qsTr("NFT collection")
            onTriggered: root.setPattern("nft")
        }
    }

    Flickable {
        id: scroll

        anchors.fill: parent
        clip: true
        contentWidth: width
        contentHeight: contentColumn.implicitHeight + 32
        flickableDirection: Flickable.VerticalFlick

        ScrollBar.vertical: ScrollBar {
            id: verticalScrollBar

            parent: scroll.parent
            anchors.top: scroll.top
            anchors.right: scroll.right
            anchors.bottom: scroll.bottom
            anchors.topMargin: 4
            anchors.rightMargin: 4
            anchors.bottomMargin: 4
            policy: scroll.contentHeight > scroll.height ? ScrollBar.AlwaysOn : ScrollBar.AlwaysOff
            active: true
            visible: policy === ScrollBar.AlwaysOn
            width: 12
            z: 10

            background: Rectangle {
                radius: 6
                color: "#292929"
            }

            contentItem: Rectangle {
                implicitWidth: 8
                implicitHeight: 32
                radius: 4
                color: verticalScrollBar.pressed ? "#F26A21" : "#9A8C81"
                opacity: 1
            }
        }

        ColumnLayout {
            id: contentColumn

            width: Math.max(240, Math.min(scroll.width - 32, 1120))
            x: Math.max(16, (scroll.width - width) / 2)
            y: 16
            spacing: 12

            ColumnLayout {
                id: pageHeader

                Layout.fillWidth: true
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    spacing: 3

                    Text {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        color: "#E7E1D8"
                        font.pixelSize: 28
                        font.weight: Font.DemiBold
                        wrapMode: Text.Wrap
                        text: qsTr("Create definition")
                    }

                    Text {
                        Layout.fillWidth: true
                        Layout.minimumWidth: 0
                        color: "#A9A098"
                        font.pixelSize: 14
                        wrapMode: Text.Wrap
                        text: qsTr("Configure the token, confirm its account targets, then create the definition.")
                    }
                }

                RowLayout {
                    id: progressRow

                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    spacing: 5

                    Repeater {
                        model: [qsTr("Configure"), qsTr("Account targets"), qsTr("Review")]

                        delegate: RowLayout {
                            id: stepItem

                            required property int index
                            required property var modelData

                            Layout.fillWidth: true
                            Layout.minimumWidth: 0
                            spacing: 5

                            Rectangle {
                                Layout.preferredWidth: 24
                                Layout.preferredHeight: 24
                                radius: 12
                                color: root.step === stepItem.index ? "#F26A21" : root.step > stepItem.index ? "#183222" : "#202020"
                                border.color: root.step === stepItem.index ? "#F26A21" : root.step > stepItem.index ? "#39C06A" : "#343434"
                                border.width: 1

                                Text {
                                    anchors.centerIn: parent
                                    color: root.step === stepItem.index ? "#151515" : root.step > stepItem.index ? "#78C88D" : "#A9A098"
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    text: (stepItem.index + 1).toString()
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                Layout.minimumWidth: 0
                                color: root.step === stepItem.index ? "#E7E1D8" : "#A9A098"
                                font.pixelSize: 13
                                font.weight: root.step === stepItem.index ? Font.DemiBold : Font.Normal
                                horizontalAlignment: Text.AlignLeft
                                wrapMode: Text.Wrap
                                text: stepItem.modelData
                                verticalAlignment: Text.AlignVCenter
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.minimumWidth: 0
                                Layout.preferredHeight: 1
                                visible: stepItem.index < 2
                                color: root.step > stepItem.index ? "#39C06A" : "#343434"
                            }
                        }
                    }
                }
            }

            GridLayout {
                id: stage

                readonly property int columnCount: contentColumn.width >= 860 ? 2 : 1

                Layout.fillWidth: true
                Layout.minimumWidth: 0
                columns: columnCount
                columnSpacing: 12
                rowSpacing: 12

                Rectangle {
                    id: mainPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.preferredWidth: stage.columnCount === 2 ? 640 : 0
                    Layout.maximumWidth: stage.columnCount === 2 ? 680 : 1120
                    implicitHeight: editor.implicitHeight + 32
                    radius: 16
                    color: "#1B1B1B"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: editor

                        anchors.fill: parent
                        anchors.margins: 14
                        spacing: 10

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 3

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 20
                                font.weight: Font.DemiBold
                                text: root.step === 0 ? qsTr("Token settings") : root.step === 1 ? qsTr("Account targets") : qsTr("Review definition")
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 13
                                wrapMode: Text.Wrap
                                text: root.step === 0 ? qsTr("Choose the fields that define this token.") : root.step === 1 ? qsTr("These accounts are created with the token definition.") : qsTr("Review the definition before creating it.")
                            }
                        }

                        ColumnLayout {
                            id: configureView

                            Layout.fillWidth: true
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            spacing: 10
                            visible: root.step === 0

                            Rectangle {
                                id: kindTabs

                                Layout.fillWidth: true
                                Layout.preferredHeight: 44
                                Layout.minimumHeight: 44
                                Layout.maximumHeight: 44
                                radius: 8
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1

                                RowLayout {
                                    anchors.fill: parent
                                    spacing: 0

                                    Button {
                                    id: fungibleTab

                                    property bool selected: root.tokenKind === 0

                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    text: qsTr("Fungible")
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Create fungible token definition")
                                    onClicked: root.selectTokenKind(0)

                                    contentItem: Text {
                                        color: fungibleTab.selected ? "#F2D8C7" : "#A9A098"
                                        font.pixelSize: 14
                                        font.weight: fungibleTab.selected ? Font.DemiBold : Font.Normal
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: fungibleTab.text
                                    }

                                    background: Rectangle {
                                        radius: 7
                                        color: fungibleTab.selected ? "#211914" : "transparent"
                                        border.color: fungibleTab.activeFocus ? "#FFB26B" : fungibleTab.selected ? "#F26A21" : "transparent"
                                        border.width: fungibleTab.activeFocus ? 2 : fungibleTab.selected ? 1 : 0
                                    }
                                    }

                                    Button {
                                    id: nonFungibleTab

                                    property bool selected: root.tokenKind === 1

                                    Layout.fillWidth: true
                                    Layout.fillHeight: true
                                    text: qsTr("Non-fungible")
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Create non-fungible token definition")
                                    onClicked: root.selectTokenKind(1)

                                    contentItem: Text {
                                        color: nonFungibleTab.selected ? "#F2D8C7" : "#A9A098"
                                        font.pixelSize: 14
                                        font.weight: nonFungibleTab.selected ? Font.DemiBold : Font.Normal
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: nonFungibleTab.text
                                    }

                                    background: Rectangle {
                                        radius: 7
                                        color: nonFungibleTab.selected ? "#211914" : "transparent"
                                        border.color: nonFungibleTab.activeFocus ? "#FFB26B" : nonFungibleTab.selected ? "#F26A21" : "transparent"
                                        border.width: nonFungibleTab.activeFocus ? 2 : nonFungibleTab.selected ? 1 : 0
                                    }
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: "#A9A098"
                                font.pixelSize: 13
                                wrapMode: Text.Wrap
                                text: root.isFungible ? qsTr("One initial holding receives the full raw supply.") : qsTr("Metadata is required. The initial master holding controls printing.")
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            Text {
                                color: "#E7E1D8"
                                font.pixelSize: 16
                                font.weight: Font.DemiBold
                                text: qsTr("Identity")
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Name")
                            }

                            TextField {
                                id: nameField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 44
                                Accessible.name: qsTr("Token definition name")
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                placeholderText: qsTr("Name stored on the definition")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true
                                text: qsTr("New token")

                                onTextEdited: root.clearDraftState()

                                background: Rectangle {
                                    radius: 7
                                    color: "#101010"
                                    border.color: nameField.activeFocus ? "#F26A21" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: root.supplyLabel
                            }

                            TextField {
                                id: supplyField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 44
                                Accessible.name: root.supplyLabel
                                color: "#E7E1D8"
                                font.pixelSize: 15
                                inputMethodHints: Qt.ImhDigitsOnly
                                placeholderText: qsTr("Unsigned 128-bit integer")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true
                                text: "1000000"
                                validator: RegularExpressionValidator {
                                    regularExpression: /^[0-9]*$/
                                }

                                onTextEdited: root.clearDraftState()

                                background: Rectangle {
                                    radius: 7
                                    color: "#101010"
                                    border.color: supplyField.activeFocus ? "#F26A21" : supplyField.text.length > 0 && !root.validSupply ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: root.validSupply ? "#A9A098" : "#F08A76"
                                font.pixelSize: 12
                                wrapMode: Text.Wrap
                                text: root.validSupply ? qsTr("Raw u128 value. Display decimals are inferred by clients, not stored here.") : qsTr("Enter an unsigned 128-bit integer between 0 and 340282366920938463463374607431768211455.")
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.isFungible
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 8

                                Rectangle {
                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 1
                                    color: "#303030"
                                }

                                Text {
                                    color: "#E7E1D8"
                                    font.pixelSize: 16
                                    font.weight: Font.DemiBold
                                    text: qsTr("Mint authority")
                                }

                                ComboBox {
                                    id: authoritySelector

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 44
                                    Accessible.name: qsTr("Mint authority policy")
                                    currentIndex: root.authorityMode
                                    model: [qsTr("Fixed supply — no future minting"), qsTr("Self authority — definition account mints"), qsTr("External authority — another account mints")]

                                    onActivated: {
                                        root.authorityMode = currentIndex;
                                        root.clearDraftState();
                                    }

                                    contentItem: Text {
                                        leftPadding: 12
                                        rightPadding: authoritySelector.indicator.width + authoritySelector.spacing
                                        color: "#E7E1D8"
                                        elide: Text.ElideRight
                                        font.pixelSize: 14
                                        text: authoritySelector.currentText
                                        verticalAlignment: Text.AlignVCenter
                                    }

                                    background: Rectangle {
                                        radius: 7
                                        color: "#101010"
                                        border.color: authoritySelector.activeFocus ? "#F26A21" : "#343434"
                                        border.width: 1
                                    }
                                }

                                TextField {
                                    id: externalAuthorityField

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: root.authorityMode === 2 ? 44 : 0
                                    Accessible.name: qsTr("External mint authority account ID")
                                    clip: true
                                    color: "#E7E1D8"
                                    font.pixelSize: 14
                                    placeholderText: qsTr("External authority account ID")
                                    placeholderTextColor: "#8E8780"
                                    selectByMouse: true
                                    visible: root.authorityMode === 2

                                    onTextEdited: root.clearDraftState()

                                    background: Rectangle {
                                        radius: 7
                                        color: "#101010"
                                        border.color: externalAuthorityField.activeFocus ? "#F26A21" : externalAuthorityField.text.length > 0 && !root.validExternalAuthority ? "#D85F4B" : "#343434"
                                        border.width: 1
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 1
                                color: "#303030"
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 10

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 3

                                        Text {
                                            color: "#E7E1D8"
                                            font.pixelSize: 16
                                            font.weight: Font.DemiBold
                                            text: qsTr("Metadata")
                                        }

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 12
                                            text: root.requiresMetadata ? qsTr("Required for non-fungible definitions") : qsTr("Optional for fungible definitions")
                                        }
                                    }

                                    CheckBox {
                                        id: metadataCheckBox

                                        Layout.alignment: Qt.AlignVCenter
                                        Accessible.name: qsTr("Include metadata account")
                                        checked: root.hasMetadata
                                        enabled: !root.requiresMetadata
                                        text: qsTr("Include")

                                        onToggled: {
                                            root.metadataEnabled = checked;
                                            root.clearDraftState();
                                        }

                                        contentItem: Text {
                                            leftPadding: metadataCheckBox.indicator.width + metadataCheckBox.spacing
                                            color: metadataCheckBox.enabled ? "#B8ADA3" : "#6D6761"
                                            font.pixelSize: 12
                                            text: metadataCheckBox.text
                                            verticalAlignment: Text.AlignVCenter
                                        }

                                        indicator: Rectangle {
                                            implicitWidth: 18
                                            implicitHeight: 18
                                            x: metadataCheckBox.leftPadding
                                            y: metadataCheckBox.topPadding + (metadataCheckBox.availableHeight - height) / 2
                                            radius: 4
                                            color: metadataCheckBox.checked ? "#F26A21" : "#101010"
                                            border.color: metadataCheckBox.activeFocus ? "#FFB26B" : metadataCheckBox.checked ? "#F26A21" : "#8E8780"
                                            border.width: metadataCheckBox.activeFocus ? 2 : 1

                                            Rectangle {
                                                anchors.centerIn: parent
                                                width: 6
                                                height: 6
                                                radius: 3
                                                color: "#151515"
                                                visible: metadataCheckBox.checked
                                            }
                                        }
                                    }
                                }

                                ColumnLayout {
                                    Layout.fillWidth: true
                                    visible: root.hasMetadata
                                    Layout.preferredHeight: visible ? implicitHeight : 0
                                    spacing: 8

                                    Text {
                                        Layout.fillWidth: true
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        wrapMode: Text.Wrap
                                        text: qsTr("Only the standard, URI, creators, definition link, and primary-sale date are stored. URI content is not validated by the program.")
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true
                                        spacing: 8

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 12
                                            text: qsTr("Standard")
                                        }

                                        ComboBox {
                                            id: metadataStandardSelector

                                            Layout.preferredWidth: 150
                                            Layout.preferredHeight: 36
                                            Accessible.name: qsTr("Metadata standard")
                                            currentIndex: root.metadataStandard
                                            model: [qsTr("Simple"), qsTr("Expanded")]

                                            onActivated: {
                                                root.metadataStandard = currentIndex;
                                                root.clearDraftState();
                                            }

                                            background: Rectangle {
                                                radius: 7
                                                color: "#101010"
                                                border.color: metadataStandardSelector.activeFocus ? "#F26A21" : "#343434"
                                                border.width: 1
                                            }
                                        }

                                        Item {
                                            Layout.fillWidth: true
                                        }
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: qsTr("URI")
                                    }

                                    TextField {
                                        id: metadataUriField

                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 44
                                        Accessible.name: qsTr("Metadata URI")
                                        color: "#E7E1D8"
                                        font.pixelSize: 14
                                        placeholderText: qsTr("data:application/json;base64,... or external URI")
                                        placeholderTextColor: "#8E8780"
                                        selectByMouse: true

                                        onTextEdited: root.clearDraftState()

                                        background: Rectangle {
                                            radius: 7
                                            color: "#101010"
                                            border.color: metadataUriField.activeFocus ? "#F26A21" : "#343434"
                                            border.width: 1
                                        }
                                    }

                                    Text {
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        text: qsTr("Creators")
                                    }

                                    TextField {
                                        id: creatorsField

                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 44
                                        Accessible.name: qsTr("Metadata creators string")
                                        color: "#E7E1D8"
                                        font.pixelSize: 14
                                        placeholderText: qsTr("Creator account or attribution string")
                                        placeholderTextColor: "#8E8780"
                                        selectByMouse: true

                                        onTextEdited: root.clearDraftState()

                                        background: Rectangle {
                                            radius: 7
                                            color: "#101010"
                                            border.color: creatorsField.activeFocus ? "#F26A21" : "#343434"
                                            border.width: 1
                                        }
                                    }
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                Button {
                                    id: examplesButton

                                    Layout.preferredHeight: 36
                                    text: qsTr("Use template")
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Choose a token definition template")
                                    onClicked: examplesMenu.popup(examplesButton, Qt.point(0, examplesButton.height))

                                    contentItem: Text {
                                        color: "#B8ADA3"
                                        font.pixelSize: 12
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: examplesButton.text
                                    }

                                    background: Rectangle {
                                        radius: 7
                                        color: "#101010"
                                        border.color: examplesButton.activeFocus ? "#FFB26B" : "#343434"
                                        border.width: examplesButton.activeFocus ? 2 : 1
                                    }
                                }

                                Item {
                                    Layout.fillWidth: true
                                }
                            }

                            Button {
                                id: continueButton

                                Layout.fillWidth: true
                                Layout.preferredHeight: visible ? 46 : 0
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Continue to account targets")
                                enabled: root.canContinue
                                text: root.canContinue ? qsTr("Continue to account targets") : qsTr("Complete required fields")
                                visible: stage.columnCount === 1
                                onClicked: root.step = 1

                                contentItem: Text {
                                    color: continueButton.enabled ? "#FFFFFF" : "#8E8780"
                                    font.pixelSize: 15
                                    font.weight: Font.DemiBold
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: continueButton.text
                                }

                                background: Rectangle {
                                    radius: 8
                                    color: continueButton.enabled ? "#F26A21" : "#282522"
                                    border.color: continueButton.activeFocus ? "#FFB26B" : continueButton.enabled ? "#F26A21" : "#3C3833"
                                    border.width: continueButton.activeFocus ? 2 : 1
                                }
                            }
                        }

                        ColumnLayout {
                            id: accountsView

                            Layout.fillWidth: true
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            spacing: 12
                            visible: root.step === 1

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: accountList.implicitHeight + 24
                                radius: 10
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1

                                ColumnLayout {
                                    id: accountList

                                    anchors.fill: parent
                                    anchors.margins: 12
                                    spacing: 6

                                    Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        text: qsTr("What this definition creates")
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        color: "#A9A098"
                                        font.pixelSize: 12
                                        wrapMode: Text.Wrap
                                        text: root.hasMetadata ? qsTr("A definition account, an initial holding, and a metadata account.") : qsTr("A definition account and an initial holding.")
                                    }
                                }
                            }

                            Button {
                                id: createAccountsButton

                                Layout.fillWidth: true
                                Layout.preferredHeight: 38
                                enabled: root.backend !== null && root.backend.isWalletOpen && !root.accountBusy
                                text: root.accountBusy ? qsTr("Creating fresh accounts…") : qsTr("Create fresh wallet accounts")
                                Accessible.name: qsTr("Create fresh wallet accounts for this definition")
                                onClicked: root.createTargetAccounts()

                                contentItem: Text {
                                    color: parent.enabled ? "#F2D8C7" : "#8E8780"
                                    font.pixelSize: 13
                                    font.weight: Font.DemiBold
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: createAccountsButton.text
                                }

                                background: Rectangle {
                                    radius: 7
                                    color: parent.enabled ? "#211914" : "#282522"
                                    border.color: parent.enabled ? "#49301F" : "#3C3833"
                                    border.width: 1
                                }
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Definition target account")
                            }

                            TextField {
                                id: definitionTargetField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 44
                                Accessible.name: qsTr("Definition target account ID")
                                clip: true
                                color: "#E7E1D8"
                                font.family: "monospace"
                                font.pixelSize: 13
                                placeholderText: qsTr("New authorized account ID")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true

                                onTextEdited: root.clearDraftState()

                                background: Rectangle {
                                    radius: 7
                                    color: "#101010"
                                    border.color: definitionTargetField.activeFocus ? "#F26A21" : definitionTargetField.text.length > 0 && !root.validDefinitionTarget ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: definitionTargetField.text.length === 0 || root.validDefinitionTarget ? "#A9A098" : "#F08A76"
                                font.pixelSize: 12
                                text: definitionTargetField.text.length === 0 || root.validDefinitionTarget ? qsTr("Fresh public wallet account; accepts base58 or 64-character hex.") : qsTr("Enter a base58 account ID or 64-character hex account ID.")
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: root.isFungible ? qsTr("Initial fungible holding target") : qsTr("NFT master holding target")
                            }

                            TextField {
                                id: holdingTargetField

                                Layout.fillWidth: true
                                Layout.preferredHeight: 44
                                Accessible.name: root.isFungible ? qsTr("Initial fungible holding target account ID") : qsTr("NFT master holding target account ID")
                                clip: true
                                color: "#E7E1D8"
                                font.family: "monospace"
                                font.pixelSize: 13
                                placeholderText: qsTr("New authorized account ID")
                                placeholderTextColor: "#8E8780"
                                selectByMouse: true

                                onTextEdited: root.clearDraftState()

                                background: Rectangle {
                                    radius: 7
                                    color: "#101010"
                                    border.color: holdingTargetField.activeFocus ? "#F26A21" : holdingTargetField.text.length > 0 && !root.validHoldingTarget ? "#D85F4B" : "#343434"
                                    border.width: 1
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                color: holdingTargetField.text.length === 0 || root.validHoldingTarget ? "#A9A098" : "#F08A76"
                                font.pixelSize: 12
                                text: holdingTargetField.text.length === 0 || root.validHoldingTarget ? qsTr("Fresh public wallet account; receives the initial state.") : qsTr("Enter a base58 account ID or 64-character hex account ID.")
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                visible: root.hasMetadata
                                Layout.preferredHeight: visible ? implicitHeight : 0
                                spacing: 7

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Metadata target account")
                                }

                                TextField {
                                    id: metadataTargetField

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 44
                                    Accessible.name: qsTr("Metadata target account ID")
                                    clip: true
                                    color: "#E7E1D8"
                                    font.family: "monospace"
                                    font.pixelSize: 13
                                    placeholderText: qsTr("New authorized account ID")
                                    placeholderTextColor: "#8E8780"
                                    selectByMouse: true

                                    onTextEdited: root.clearDraftState()

                                    background: Rectangle {
                                        radius: 7
                                        color: "#101010"
                                        border.color: metadataTargetField.activeFocus ? "#F26A21" : metadataTargetField.text.length > 0 && !root.validMetadataTarget ? "#D85F4B" : "#343434"
                                        border.width: 1
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: metadataTargetField.text.length === 0 || root.validMetadataTarget ? "#A9A098" : "#F08A76"
                                    font.pixelSize: 12
                                    text: metadataTargetField.text.length === 0 || root.validMetadataTarget ? qsTr("Fresh public wallet account; stores the metadata record.") : qsTr("Enter a base58 account ID or 64-character hex account ID.")
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                Button {
                                    id: backToConfigureButton

                                    Layout.preferredWidth: 112
                                    Layout.preferredHeight: 46
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Back to token settings")
                                    text: qsTr("Back")
                                    onClicked: root.step = 0

                                    contentItem: Text {
                                        color: "#E7E1D8"
                                        font.pixelSize: 14
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: backToConfigureButton.text
                                    }

                                    background: Rectangle {
                                        radius: 8
                                        color: "#101010"
                                        border.color: backToConfigureButton.activeFocus ? "#FFB26B" : "#343434"
                                        border.width: backToConfigureButton.activeFocus ? 2 : 1
                                    }
                                }

                                Button {
                                    id: reviewButton

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 46
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Review token definition")
                                    enabled: root.canPrepare
                                    text: root.canPrepare ? qsTr("Review definition") : qsTr("Complete targets")
                                    onClicked: root.step = 2

                                    contentItem: Text {
                                        color: reviewButton.enabled ? "#FFFFFF" : "#8E8780"
                                        elide: Text.ElideRight
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: reviewButton.text
                                    }

                                    background: Rectangle {
                                        radius: 8
                                        color: reviewButton.enabled ? "#F26A21" : "#282522"
                                        border.color: reviewButton.activeFocus ? "#FFB26B" : reviewButton.enabled ? "#F26A21" : "#3C3833"
                                        border.width: reviewButton.activeFocus ? 2 : 1
                                    }
                                }
                            }
                        }

                        ColumnLayout {
                            id: reviewView

                            Layout.fillWidth: true
                            Layout.preferredHeight: visible ? implicitHeight : 0
                            spacing: 12
                            visible: root.step === 2

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: reviewContent.implicitHeight + 24
                                radius: 10
                                color: "#101010"
                                border.color: "#343434"
                                border.width: 1

                                ColumnLayout {
                                    id: reviewContent

                                    anchors.fill: parent
                                    anchors.margins: 12
                                    spacing: 9

                                    RowLayout {
                                        Layout.fillWidth: true

                                        Rectangle {
                                            Layout.preferredWidth: 78
                                            Layout.preferredHeight: 28
                                            radius: 14
                                            color: root.isFungible ? "#211914" : "#182534"
                                            border.color: root.isFungible ? "#6A4329" : "#40607A"
                                            border.width: 1

                                            Text {
                                                anchors.centerIn: parent
                                                color: root.isFungible ? "#F2D8C7" : "#BFD8F4"
                                                font.pixelSize: 12
                                                font.weight: Font.DemiBold
                                                text: root.isFungible ? qsTr("Fungible") : qsTr("NFT")
                                            }
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            color: "#E7E1D8"
                                            elide: Text.ElideRight
                                            font.pixelSize: 18
                                            font.weight: Font.DemiBold
                                            text: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition")
                                        }
                                    }

                                    Rectangle {
                                        Layout.fillWidth: true
                                        Layout.preferredHeight: 1
                                        color: "#303030"
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 13
                                            text: root.supplyLabel
                                        }

                                        Item {
                                            Layout.fillWidth: true
                                        }

                                        Text {
                                            color: root.validSupply ? "#E7E1D8" : "#F08A76"
                                            font.pixelSize: 14
                                            font.weight: Font.Medium
                                            text: root.supplyValue
                                        }
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 13
                                            text: root.isFungible ? qsTr("Mint authority") : qsTr("Printing")
                                        }

                                        Item {
                                            Layout.fillWidth: true
                                        }

                                        Text {
                                            Layout.maximumWidth: 280
                                            color: "#E7E1D8"
                                            elide: Text.ElideRight
                                            font.pixelSize: 13
                                            text: root.authorityValue
                                            horizontalAlignment: Text.AlignRight
                                        }
                                    }

                                    RowLayout {
                                        Layout.fillWidth: true

                                        Text {
                                            color: "#A9A098"
                                            font.pixelSize: 13
                                            text: qsTr("Metadata")
                                        }

                                        Item {
                                            Layout.fillWidth: true
                                        }

                                        Text {
                                            color: "#E7E1D8"
                                            font.pixelSize: 13
                                            text: root.hasMetadata ? root.metadataStandard === 0 ? qsTr("Simple") : qsTr("Expanded") : qsTr("None")
                                        }
                                    }
                                }
                            }

                            Text {
                                color: "#A9A098"
                                font.pixelSize: 12
                                text: qsTr("Account targets")
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 6

                                Repeater {
                                    model: [
                                        { label: qsTr("Definition"), value: definitionTargetField.text, ok: root.validDefinitionTarget, metadata: false },
                                        { label: root.isFungible ? qsTr("Initial holding") : qsTr("Master holding"), value: holdingTargetField.text, ok: root.validHoldingTarget, metadata: false },
                                        { label: qsTr("Metadata"), value: metadataTargetField.text, ok: root.validMetadataTarget, metadata: true }
                                    ]

                                    delegate: RowLayout {
                                        id: reviewTargetRow

                                        required property var modelData

                                        Layout.fillWidth: true
                                        visible: !reviewTargetRow.modelData.metadata || root.hasMetadata
                                        spacing: 8

                                        Text {
                                            Layout.preferredWidth: 112
                                            color: "#A9A098"
                                            font.pixelSize: 12
                                            text: reviewTargetRow.modelData.label
                                        }

                                        Text {
                                            Layout.fillWidth: true
                                            color: "#E7E1D8"
                                            elide: Text.ElideMiddle
                                            font.family: "monospace"
                                            font.pixelSize: 12
                                            text: reviewTargetRow.modelData.value.length > 0 ? reviewTargetRow.modelData.value : qsTr("Missing")
                                        }

                                        Text {
                                            color: reviewTargetRow.modelData.value.length > 0 && reviewTargetRow.modelData.ok ? "#78C88D" : "#F08A76"
                                            font.pixelSize: 12
                                            text: reviewTargetRow.modelData.value.length > 0 && reviewTargetRow.modelData.ok ? qsTr("Ready") : qsTr("Needs input")
                                        }
                                    }
                                }
                            }

                            Rectangle {
                                Layout.fillWidth: true
                                implicitHeight: reviewNotice.implicitHeight + 20
                                radius: 8
                                color: "#211914"
                                border.color: "#49301F"
                                border.width: 1

                                Text {
                                    id: reviewNotice

                                    anchors.fill: parent
                                    anchors.margins: 10
                                    color: "#F2D8C7"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: root.hasMetadata ? qsTr("Metadata-backed definitions use typed serialization.") : qsTr("Fungible definitions use the standard Token Program fields.")
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                visible: root.prepared || root.errorMessage.length > 0
                                color: root.errorMessage.length > 0 ? "#F08A76" : "#78C88D"
                                font.pixelSize: 13
                                wrapMode: Text.Wrap
                                text: root.errorMessage.length > 0 ? root.errorMessage : root.preparedMessage
                            }

                            RowLayout {
                                Layout.fillWidth: true
                                spacing: 8

                                Button {
                                    id: backToAccountsButton

                                    Layout.preferredWidth: 112
                                    Layout.preferredHeight: 46
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Back to account targets")
                                    enabled: !root.prepared && !root.submitting
                                    text: qsTr("Back")
                                    onClicked: root.step = 1

                                    contentItem: Text {
                                        color: backToAccountsButton.enabled ? "#E7E1D8" : "#8E8780"
                                        font.pixelSize: 14
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: backToAccountsButton.text
                                    }

                                    background: Rectangle {
                                        radius: 8
                                        color: "#101010"
                                        border.color: backToAccountsButton.activeFocus ? "#FFB26B" : "#343434"
                                        border.width: backToAccountsButton.activeFocus ? 2 : 1
                                    }
                                }

                                Button {
                                    id: prepareButton

                                    Layout.fillWidth: true
                                    Layout.preferredHeight: 46
                                    activeFocusOnTab: true
                                    Accessible.name: qsTr("Create token definition")
                                    enabled: root.canSubmit
                                    text: root.submitting ? qsTr("Submitting…") : root.prepared ? qsTr("Transaction submitted") : !root.backend || !root.backend.isWalletOpen ? qsTr("Connect wallet to create") : root.canPrepare ? qsTr("Create token definition") : qsTr("Complete required fields")
                                    onClicked: root.prepareDefinition()

                                    contentItem: Text {
                                        color: prepareButton.enabled ? "#FFFFFF" : root.prepared ? "#78C88D" : "#8E8780"
                                        font.pixelSize: 15
                                        font.weight: Font.DemiBold
                                        horizontalAlignment: Text.AlignHCenter
                                        verticalAlignment: Text.AlignVCenter
                                        text: prepareButton.text
                                    }

                                    background: Rectangle {
                                        radius: 8
                                        color: prepareButton.enabled ? "#F26A21" : root.prepared ? "#183222" : "#282522"
                                        border.color: prepareButton.activeFocus ? "#FFB26B" : prepareButton.enabled ? "#F26A21" : root.prepared ? "#39C06A" : "#3C3833"
                                        border.width: prepareButton.activeFocus ? 2 : 1
                                    }
                                }
                            }

                            Button {
                                id: inspectButton

                                Layout.fillWidth: true
                                Layout.preferredHeight: 42
                                visible: root.prepared
                                activeFocusOnTab: true
                                Accessible.name: qsTr("Inspect token definition")
                                text: qsTr("Open in Inspect")
                                onClicked: root.requestInspect()

                                contentItem: Text {
                                    color: "#F2D8C7"
                                    font.pixelSize: 14
                                    font.weight: Font.DemiBold
                                    horizontalAlignment: Text.AlignHCenter
                                    verticalAlignment: Text.AlignVCenter
                                    text: inspectButton.text
                                }

                                background: Rectangle {
                                    radius: 8
                                    color: "#211914"
                                    border.color: inspectButton.activeFocus ? "#FFB26B" : "#49301F"
                                    border.width: inspectButton.activeFocus ? 2 : 1
                                }
                            }
                        }
                    }
                }

                Rectangle {
                    id: summaryPanel

                    Layout.alignment: Qt.AlignTop
                    Layout.fillWidth: true
                    Layout.minimumWidth: 0
                    Layout.preferredWidth: stage.columnCount === 2 ? 300 : 0
                    Layout.maximumWidth: 1120
                    implicitHeight: summaryContent.implicitHeight + 32
                    radius: 16
                    color: "#181818"
                    border.color: "#303030"
                    border.width: 1

                    ColumnLayout {
                        id: summaryContent

                        anchors.fill: parent
                        anchors.margins: 16
                        spacing: 12

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 18
                            font.weight: Font.DemiBold
                            text: qsTr("Definition summary")
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            implicitHeight: summaryHeadline.implicitHeight + 20
                            radius: 10
                            color: root.isFungible ? "#211914" : "#182534"
                            border.color: root.isFungible ? "#49301F" : "#31435D"
                            border.width: 1

                            ColumnLayout {
                                id: summaryHeadline

                                anchors.fill: parent
                                anchors.margins: 10
                                spacing: 5

                                Text {
                                    color: root.isFungible ? "#F2D8C7" : "#BFD8F4"
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    text: root.isFungible ? qsTr("Fungible") : qsTr("Non-fungible")
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#E7E1D8"
                                    elide: Text.ElideRight
                                    font.pixelSize: 18
                                    font.weight: Font.DemiBold
                                    text: nameField.text.length > 0 ? nameField.text : qsTr("Untitled definition")
                                }
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.supplyLabel
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    color: root.validSupply ? "#E7E1D8" : "#F08A76"
                                    font.pixelSize: 13
                                    font.weight: Font.Medium
                                    text: root.supplyValue
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: root.isFungible ? qsTr("Authority") : qsTr("Printing")
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    Layout.maximumWidth: 170
                                    color: "#E7E1D8"
                                    elide: Text.ElideRight
                                    font.pixelSize: 12
                                    text: root.authorityValue
                                    horizontalAlignment: Text.AlignRight
                                }
                            }

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    text: qsTr("Metadata")
                                }

                                Item {
                                    Layout.fillWidth: true
                                }

                                Text {
                                    color: "#E7E1D8"
                                    font.pixelSize: 12
                                    text: root.hasMetadata ? root.metadataStandard === 0 ? qsTr("Simple") : qsTr("Expanded") : qsTr("Not included")
                                }
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.preferredHeight: 1
                            color: "#303030"
                        }

                        Text {
                            color: "#E7E1D8"
                            font.pixelSize: 14
                            font.weight: Font.DemiBold
                            text: qsTr("Readiness")
                        }

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            Rectangle {
                                Layout.preferredWidth: 28
                                Layout.preferredHeight: 28
                                radius: 14
                                color: root.validTargetCount === root.targetCount ? "#183222" : "#332521"
                                border.color: root.validTargetCount === root.targetCount ? "#39C06A" : "#D85F4B"
                                border.width: 1

                                Text {
                                    anchors.centerIn: parent
                                    color: root.validTargetCount === root.targetCount ? "#78C88D" : "#F08A76"
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    text: root.validTargetCount === root.targetCount ? qsTr("OK") : qsTr("!")
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2

                                Text {
                                    color: "#E7E1D8"
                                    font.pixelSize: 13
                                    font.weight: Font.Medium
                                    text: root.validTargetCount === root.targetCount ? qsTr("All targets look valid") : qsTr("%1 of %2 targets ready").arg(root.validTargetCount).arg(root.targetCount)
                                }

                                Text {
                                    Layout.fillWidth: true
                                    color: "#A9A098"
                                    font.pixelSize: 12
                                    wrapMode: Text.Wrap
                                    text: root.step === 0 ? qsTr("Account targets appear next.") : root.step === 1 ? qsTr("Review unlocks when every target is valid.") : root.prepared ? qsTr("Transaction submitted to the Token Program.") : qsTr("Ready for final review.")
                                }
                            }
                        }

                        Button {
                            id: summaryContinueButton

                            Layout.fillWidth: true
                            Layout.preferredHeight: visible ? 46 : 0
                            visible: root.step === 0 && stage.columnCount === 2
                            activeFocusOnTab: true
                            Accessible.name: qsTr("Continue to account targets")
                            enabled: root.canContinue
                            text: root.canContinue ? qsTr("Continue to account targets") : qsTr("Complete required fields")
                            onClicked: root.step = 1

                            contentItem: Text {
                                color: summaryContinueButton.enabled ? "#FFFFFF" : "#8E8780"
                                font.pixelSize: 14
                                font.weight: Font.DemiBold
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                                text: summaryContinueButton.text
                            }

                            background: Rectangle {
                                radius: 8
                                color: summaryContinueButton.enabled ? "#F26A21" : "#282522"
                                border.color: summaryContinueButton.activeFocus ? "#FFB26B" : summaryContinueButton.enabled ? "#F26A21" : "#3C3833"
                                border.width: summaryContinueButton.activeFocus ? 2 : 1
                            }
                        }

                    }
                }
            }
        }
    }
}
