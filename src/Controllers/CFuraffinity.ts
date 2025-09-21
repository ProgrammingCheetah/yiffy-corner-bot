import { IImageSite, IImageSiteAll } from "../Interfaces";

class CFuraffinity implements IImageSite {
    private url: string;
    constructor(options: IImageSiteAll.IImageSiteConstructor) {
        this.url = options.site;
    }

    getDescriptions = (): string => {
        return '';
    };

    getOriginalSource = (): string => {
        return this.url;
    };

    getSource = (): string => {
        return this.url;
    };

    getStaticURL = (): string => {
        return '';

    };

    getTags = (): string[] => {
        return [];

    };

    getTitle = (): string => {
        return '';
    };
}


function GetComponent(site: string): IImageSite {
    return new CFuraffinity({ site });
}
export default { GetComponent };