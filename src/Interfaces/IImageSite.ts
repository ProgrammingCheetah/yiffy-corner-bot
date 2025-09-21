export interface IImageSiteConstructor {
    site: string;
}

abstract class IImageSite {
    abstract getStaticURL(): string;
    abstract getSource(): string;
    abstract getTags(): string[];
    abstract getDescriptions(): string;
    abstract getTitle(): string;
    abstract getOriginalSource(): string;
}

export default IImageSite;