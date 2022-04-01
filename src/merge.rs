pub trait Merge {
    type Item;

    fn merge(left: &Self::Item, right: &Self::Item) -> Self::Item;

    fn merge_peaks(peak1: &Self::Item, peak2: &Self::Item) -> Self::Item {
        Self::merge(peak1, peak2)
    }
}
